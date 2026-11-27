//! JSON-RPC 2.0 envelope handling.
//!
//! Correctness rules (Solana / Helius):
//!
//! * The response `id` must equal the request `id`; anything else is a
//!   decode error, never silently accepted.
//! * Errors arrive inside HTTP 200 as `{"error": {code, message}}`.
//! * `"result": null` is a legitimate answer ("not found" / "not yet
//!   confirmed") and is distinct from a missing `result`.
//! * Helius signals rate limiting as HTTP 429 or JSON-RPC
//!   `-32005 "Too many requests"`; Solana uses `-32005` for "node unhealthy",
//!   so the message disambiguates.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use serde_json::{Value, json};
use solprint_config::Secret;
use url::Url;

use crate::context::ProviderId;
use crate::error::{GatewayError, sanitize_message};
use crate::operations::PROHIBITED_RPC_METHODS;
use crate::transport::{HttpRequest, HttpResponse, HttpTransport, Method};

#[derive(Debug, Default)]
pub struct IdSource(AtomicU64);

impl IdSource {
    pub fn next(&self) -> u64 {
        self.0.fetch_add(1, Ordering::Relaxed).wrapping_add(1)
    }
}

#[must_use]
pub fn request_body(id: u64, method: &str, params: &Value) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params})
}

/// Map a non-success HTTP status to an error (429 → rate limited).
pub fn check_status(provider: ProviderId, resp: &HttpResponse) -> Result<(), GatewayError> {
    match resp.status {
        200..=299 => Ok(()),
        429 => Err(GatewayError::RateLimited {
            provider,
            retry_after: resp.retry_after,
        }),
        status => Err(GatewayError::Http { provider, status }),
    }
}

/// Interpret one JSON-RPC response object for request `id`.
pub fn parse_response(provider: ProviderId, id: u64, v: &Value) -> Result<Value, GatewayError> {
    let obj = v.as_object().ok_or(GatewayError::Decode {
        provider,
        detail: "JSON-RPC response is not an object",
    })?;
    if obj.get("id").and_then(Value::as_u64) != Some(id) {
        // A server-level error (e.g. parse error) may carry `id: null`.
        if let Some(err) = obj.get("error") {
            return Err(rpc_error(provider, err));
        }
        return Err(GatewayError::Decode {
            provider,
            detail: "JSON-RPC response id mismatch",
        });
    }
    if let Some(err) = obj.get("error") {
        return Err(rpc_error(provider, err));
    }
    obj.get("result").cloned().ok_or(GatewayError::Decode {
        provider,
        detail: "JSON-RPC response has neither result nor error",
    })
}

fn rpc_error(provider: ProviderId, err: &Value) -> GatewayError {
    let code = err.get("code").and_then(Value::as_i64).unwrap_or(0);
    let message = err.get("message").and_then(Value::as_str).unwrap_or("");
    if code == -32005 && message.to_ascii_lowercase().contains("too many") {
        return GatewayError::RateLimited {
            provider,
            retry_after: None,
        };
    }
    if message.contains("Transaction version") && message.contains("not supported") {
        return GatewayError::UnsupportedTransactionVersion;
    }
    GatewayError::Rpc {
        provider,
        code,
        message: sanitize_message(message),
    }
}

/// Endpoint plus credential, kept secret together because Helius carries the
/// key in the query string.
#[derive(Debug, Clone)]
pub struct RpcEndpoint {
    pub provider: ProviderId,
    pub url: Secret<Url>,
}

/// Send one JSON-RPC call.
pub async fn call(
    transport: &HttpTransport,
    ids: &IdSource,
    endpoint: &RpcEndpoint,
    method: &'static str,
    params: Value,
    timeout: Duration,
) -> Result<Value, GatewayError> {
    if PROHIBITED_RPC_METHODS.contains(&method) {
        return Err(GatewayError::Prohibited(
            "transaction submission methods are not available",
        ));
    }
    let id = ids.next();
    let body = serde_json::to_vec(&request_body(id, method, &params))
        .map_err(|_| GatewayError::InvalidRequest("request could not be encoded"))?;
    let resp = transport
        .send(HttpRequest {
            provider: endpoint.provider,
            method: Method::Post,
            url: endpoint.url.clone(),
            headers: Vec::new(),
            body: Some(body),
            timeout,
        })
        .await?;
    check_status(endpoint.provider, &resp)?;
    let v: Value = serde_json::from_slice(&resp.body).map_err(|_| GatewayError::Decode {
        provider: endpoint.provider,
        detail: "response is not JSON",
    })?;
    parse_response(endpoint.provider, id, &v)
}

/// Send a JSON-RPC batch of the same method. Results are returned in request
/// order regardless of response order; a missing entry is a decode error
/// for that item only.
pub async fn batch(
    transport: &HttpTransport,
    ids: &IdSource,
    endpoint: &RpcEndpoint,
    method: &'static str,
    params: Vec<Value>,
    timeout: Duration,
) -> Result<Vec<Result<Value, GatewayError>>, GatewayError> {
    if PROHIBITED_RPC_METHODS.contains(&method) {
        return Err(GatewayError::Prohibited(
            "transaction submission methods are not available",
        ));
    }
    let with_ids: Vec<(u64, Value)> = params.into_iter().map(|p| (ids.next(), p)).collect();
    let body: Vec<Value> = with_ids
        .iter()
        .map(|(id, p)| request_body(*id, method, p))
        .collect();
    let body = serde_json::to_vec(&body)
        .map_err(|_| GatewayError::InvalidRequest("request could not be encoded"))?;
    let resp = transport
        .send(HttpRequest {
            provider: endpoint.provider,
            method: Method::Post,
            url: endpoint.url.clone(),
            headers: Vec::new(),
            body: Some(body),
            timeout,
        })
        .await?;
    check_status(endpoint.provider, &resp)?;
    let v: Value = serde_json::from_slice(&resp.body).map_err(|_| GatewayError::Decode {
        provider: endpoint.provider,
        detail: "response is not JSON",
    })?;
    let items = match v {
        Value::Array(items) => items,
        // A whole-batch failure comes back as a single error object.
        Value::Object(_) => {
            return Err(parse_response(endpoint.provider, 0, &v).err().unwrap_or(
                GatewayError::Decode {
                    provider: endpoint.provider,
                    detail: "batch response is not an array",
                },
            ));
        }
        _ => {
            return Err(GatewayError::Decode {
                provider: endpoint.provider,
                detail: "batch response is not an array",
            });
        }
    };
    Ok(with_ids
        .iter()
        .map(|(id, _)| {
            items
                .iter()
                .find(|item| item.get("id").and_then(Value::as_u64) == Some(*id))
                .map_or(
                    Err(GatewayError::Decode {
                        provider: endpoint.provider,
                        detail: "batch response is missing an item",
                    }),
                    |item| parse_response(endpoint.provider, *id, item),
                )
        })
        .collect())
}

/// Split `{context: {slot}, value}` responses.
#[must_use]
pub fn split_context(result: Value) -> (Option<u64>, Value) {
    match result {
        Value::Object(mut map) if map.contains_key("context") && map.contains_key("value") => {
            let slot = map
                .get("context")
                .and_then(|c| c.get("slot"))
                .and_then(Value::as_u64);
            (slot, map.remove("value").unwrap_or(Value::Null))
        }
        other => (None, other),
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    const P: ProviderId = ProviderId::Helius;

    #[test]
    fn id_must_match() {
        let v = json!({"jsonrpc": "2.0", "id": 7, "result": 1});
        assert_eq!(parse_response(P, 7, &v).unwrap(), json!(1));
        assert!(matches!(
            parse_response(P, 8, &v),
            Err(GatewayError::Decode { .. })
        ));
    }

    #[test]
    fn null_result_is_an_answer_but_missing_result_is_not() {
        let v = json!({"jsonrpc": "2.0", "id": 1, "result": null});
        assert_eq!(parse_response(P, 1, &v).unwrap(), Value::Null);
        let v = json!({"jsonrpc": "2.0", "id": 1});
        assert!(parse_response(P, 1, &v).is_err());
    }

    #[test]
    fn errors_inside_http_200_are_typed_and_sanitized() {
        let v = json!({"jsonrpc":"2.0","id":1,"error":{"code":-32602,"message":"Invalid param: https://x/?api-key=SECRET bad"}});
        let GatewayError::Rpc { code, message, .. } = parse_response(P, 1, &v).unwrap_err() else {
            panic!()
        };
        assert_eq!(code, -32602);
        assert!(!message.contains("SECRET"));

        let v =
            json!({"jsonrpc":"2.0","id":1,"error":{"code":-32005,"message":"Too many requests"}});
        assert!(matches!(
            parse_response(P, 1, &v),
            Err(GatewayError::RateLimited { .. })
        ));
        let v = json!({"jsonrpc":"2.0","id":1,"error":{"code":-32005,"message":"Node is behind by 42 slots"}});
        assert!(matches!(
            parse_response(P, 1, &v),
            Err(GatewayError::Rpc { code: -32005, .. })
        ));
        let v = json!({"jsonrpc":"2.0","id":null,"error":{"code":-32700,"message":"Parse error"}});
        assert!(matches!(
            parse_response(P, 1, &v),
            Err(GatewayError::Rpc { code: -32700, .. })
        ));
        let v = json!({"jsonrpc":"2.0","id":1,"error":{"code":-32015,"message":"Transaction version (1) is not supported by the requesting client."}});
        assert_eq!(
            parse_response(P, 1, &v).unwrap_err(),
            GatewayError::UnsupportedTransactionVersion
        );
    }

    #[test]
    fn context_is_split_from_value() {
        let (slot, v) =
            split_context(json!({"context": {"slot": 9, "apiVersion": "2.0"}, "value": 5}));
        assert_eq!(slot, Some(9));
        assert_eq!(v, json!(5));
        let (slot, v) = split_context(json!(5));
        assert_eq!((slot, v), (None, json!(5)));
    }
}
