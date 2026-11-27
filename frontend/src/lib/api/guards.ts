/**
 * Minimal runtime contract guards. The TypeScript types are generated from
 * Rust; these checks make sure a response actually has the load-bearing
 * fields before the UI trusts it (a proxy error page, an old backend or a
 * truncated body must never render as "healthy").
 */
import type {
  GatewayBudgetsResponse,
  GatewayCacheResponse,
  GatewayMetricsResponse,
  GatewayProvidersResponse,
  GatewayStatus,
  GatewayStreamsResponse,
  GatewayUsageResponse,
  ReadyResponse,
  SystemStatus,
  VersionInfo,
} from '@/types/api.ts';

type Obj = Record<string, unknown>;

const isObj = (v: unknown): v is Obj => typeof v === 'object' && v !== null && !Array.isArray(v);

export function isSystemStatus(v: unknown): v is SystemStatus {
  if (!isObj(v) || !isObj(v.trading) || !isObj(v.database) || !Array.isArray(v.providers)) {
    return false;
  }
  return (
    v.application === 'SOLPRINT' &&
    (v.trading.mode === 'PAPER' || v.trading.mode === 'LIVE') &&
    typeof v.trading.live_trading_enabled === 'boolean' &&
    typeof v.timestamp === 'string' &&
    typeof v.uptime_seconds === 'number' &&
    (v.database.status === 'AVAILABLE' || v.database.status === 'UNAVAILABLE')
  );
}

export function isReadyResponse(v: unknown): v is ReadyResponse {
  return (
    isObj(v) &&
    (v.status === 'READY' || v.status === 'NOT_READY') &&
    isObj(v.checks) &&
    isObj(v.checks.database)
  );
}

export function isVersionInfo(v: unknown): v is VersionInfo {
  return (
    isObj(v) &&
    v.name === 'solprint' &&
    typeof v.version === 'string' &&
    typeof v.api_version === 'string'
  );
}

const isStr = (v: unknown): v is string => typeof v === 'string';
const isNum = (v: unknown): v is number => typeof v === 'number' && Number.isFinite(v);

export function isGatewayStatus(v: unknown): v is GatewayStatus {
  return (
    isObj(v) &&
    typeof v.enabled === 'boolean' &&
    v.execution_capability === 'NONE' &&
    isStr(v.generated_at) &&
    Array.isArray(v.providers) &&
    isObj(v.streams) &&
    isObj(v.persistence) &&
    isObj(v.scheduler) &&
    isObj(v.cache) &&
    (v.budget === null || (isObj(v.budget) && isNum(v.budget.credits_used)))
  );
}

export function isGatewayProviders(v: unknown): v is GatewayProvidersResponse {
  return (
    isObj(v) &&
    isStr(v.generated_at) &&
    Array.isArray(v.providers) &&
    v.providers.every((p) => isObj(p) && isObj(p.health) && isObj(p.circuit) && isObj(p.traffic))
  );
}

export function isGatewayBudgets(v: unknown): v is GatewayBudgetsResponse {
  return (
    isObj(v) &&
    isStr(v.generated_at) &&
    Array.isArray(v.budgets) &&
    v.budgets.every((b) => isObj(b) && isNum(b.credits_used) && isNum(b.credit_limit))
  );
}

export function isGatewayMetrics(v: unknown): v is GatewayMetricsResponse {
  return isObj(v) && isStr(v.generated_at) && Array.isArray(v.operations);
}

export function isGatewayStreams(v: unknown): v is GatewayStreamsResponse {
  return (
    isObj(v) &&
    isStr(v.generated_at) &&
    typeof v.enabled === 'boolean' &&
    Array.isArray(v.subscriptions) &&
    Array.isArray(v.checkpoints)
  );
}

export function isGatewayCache(v: unknown): v is GatewayCacheResponse {
  return isObj(v) && isStr(v.generated_at) && isNum(v.entries) && Array.isArray(v.namespaces);
}

export function isGatewayUsage(v: unknown): v is GatewayUsageResponse {
  return (
    isObj(v) &&
    isStr(v.since) &&
    Array.isArray(v.process) &&
    Array.isArray(v.history) &&
    typeof v.history_available === 'boolean'
  );
}
