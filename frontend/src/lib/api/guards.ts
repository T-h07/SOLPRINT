/**
 * Minimal runtime contract guards. The TypeScript types are generated from
 * Rust; these checks make sure a response actually has the load-bearing
 * fields before the UI trusts it (a proxy error page, an old backend or a
 * truncated body must never render as "healthy").
 */
import type { ReadyResponse, SystemStatus, VersionInfo } from '@/types/api.ts';

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
