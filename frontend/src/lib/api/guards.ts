/**
 * Minimal runtime contract guards. The TypeScript types are generated from
 * Rust; these checks make sure a response actually has the load-bearing
 * fields before the UI trusts it (a proxy error page, an old backend or a
 * truncated body must never render as "healthy").
 */
import type {
  DataBackfillsResponse,
  DataDecodersResponse,
  DataEngineStatus,
  DataEventsResponse,
  DataIssuesResponse,
  DataSwapsResponse,
  DataTargetsResponse,
  DataTransactionDetail,
  DataTransactionsResponse,
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

// DATA ENGINE (Phase 02). Amounts are strings; a number where a string is
// expected means the contract changed and the value must not be rendered.

const isStrOrNull = (v: unknown): v is string | null => v === null || typeof v === 'string';

export function isDataStatus(v: unknown): v is DataEngineStatus {
  return (
    isObj(v) &&
    typeof v.running === 'boolean' &&
    v.execution_capability === 'NONE' &&
    isStr(v.decoder_version) &&
    isStr(v.generated_at) &&
    typeof v.counts_available === 'boolean' &&
    Array.isArray(v.open_issues_by_severity) &&
    (v.queue === null || (isObj(v.queue) && isNum(v.queue.depth) && isNum(v.queue.capacity))) &&
    (v.transactions === null || (isObj(v.transactions) && isNum(v.transactions.total)))
  );
}

function isSwap(v: unknown): boolean {
  return (
    isObj(v) &&
    isStr(v.swap_id) &&
    isStr(v.signature) &&
    isObj(v.input) &&
    isObj(v.output) &&
    isStrOrNull(v.input.amount) &&
    isStrOrNull(v.output.amount) &&
    isStrOrNull(v.exchange_ratio) &&
    isStr(v.attribution) &&
    isStr(v.classification_status) &&
    Array.isArray(v.legs) &&
    Array.isArray(v.fees) &&
    isObj(v.valuation)
  );
}

function isTxSummary(v: unknown): boolean {
  return (
    isObj(v) &&
    isStr(v.signature) &&
    isNum(v.slot) &&
    isStr(v.fee_lamports) &&
    isStr(v.finality) &&
    isStr(v.decode_status) &&
    Array.isArray(v.protocols)
  );
}

export function isDataTransactions(v: unknown): v is DataTransactionsResponse {
  return (
    isObj(v) &&
    isStr(v.generated_at) &&
    Array.isArray(v.items) &&
    v.items.every(isTxSummary) &&
    isStrOrNull(v.next_cursor)
  );
}

export function isDataTransactionDetail(v: unknown): v is DataTransactionDetail {
  return (
    isObj(v) &&
    isStr(v.generated_at) &&
    isTxSummary(v.summary) &&
    Array.isArray(v.evidence) &&
    Array.isArray(v.balances) &&
    Array.isArray(v.activities) &&
    Array.isArray(v.swaps) &&
    v.swaps.every(isSwap) &&
    Array.isArray(v.issues)
  );
}

export function isDataSwaps(v: unknown): v is DataSwapsResponse {
  return (
    isObj(v) &&
    isStr(v.generated_at) &&
    Array.isArray(v.items) &&
    v.items.every(isSwap) &&
    isStrOrNull(v.next_cursor)
  );
}

export function isDataIssues(v: unknown): v is DataIssuesResponse {
  return isObj(v) && isStr(v.generated_at) && Array.isArray(v.items);
}

export function isDataTargets(v: unknown): v is DataTargetsResponse {
  return isObj(v) && isStr(v.generated_at) && isNum(v.max_active_targets) && Array.isArray(v.items);
}

export function isDataBackfills(v: unknown): v is DataBackfillsResponse {
  return isObj(v) && isStr(v.generated_at) && Array.isArray(v.items);
}

export function isDataEvents(v: unknown): v is DataEventsResponse {
  return isObj(v) && isStr(v.generated_at) && Array.isArray(v.items);
}

export function isDataDecoders(v: unknown): v is DataDecodersResponse {
  return (
    isObj(v) &&
    isStr(v.decoder_version) &&
    isStr(v.network) &&
    Array.isArray(v.decoders) &&
    v.decoders.every((d) => isObj(d) && isStr(d.id) && isStr(d.protocol) && isStr(d.program_id))
  );
}
