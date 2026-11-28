/**
 * Display vocabulary for RADAR states (values defined by the backend, see
 * docs/architecture/RADAR.md). Every state maps to a tone *and* a text label;
 * unknown values are rendered verbatim with a neutral tone, never guessed
 * into a known state.
 *
 * Tones are deliberately sparse: `positive` is reserved for states that are
 * complete and verified, never for "good trader". A priority or a
 * qualification is an investigation state, not a recommendation.
 */
import type { StateLabel } from '@/features/data/labels.ts';

function lookup(table: Record<string, StateLabel>, value: string): StateLabel {
  return table[value] ?? { tone: 'neutral', label: value.replaceAll('_', ' ').toLowerCase() };
}

const LIFECYCLE: Record<string, StateLabel> = {
  DISCOVERED: { tone: 'neutral', label: 'Discovered' },
  SCREENING: { tone: 'neutral', label: 'Screening' },
  HISTORY_QUEUED: { tone: 'info', label: 'History queued' },
  INVESTIGATING: { tone: 'info', label: 'Investigating' },
  INSUFFICIENT_EVIDENCE: { tone: 'neutral', label: 'Insufficient evidence' },
  PRELIMINARY_QUALIFIED: { tone: 'info', label: 'Preliminary qualified' },
  PRIORITIZED: { tone: 'info', label: 'Prioritized' },
  MONITORED: { tone: 'positive', label: 'Monitored' },
  DORMANT: { tone: 'neutral', label: 'Dormant' },
  REJECTED: { tone: 'neutral', label: 'Rejected' },
  ARCHIVED: { tone: 'neutral', label: 'Archived' },
};

const ELIGIBILITY: Record<string, StateLabel> = {
  ELIGIBLE_CANDIDATE: { tone: 'neutral', label: 'Eligible' },
  PROVISIONAL_CANDIDATE: { tone: 'neutral', label: 'Provisional' },
  NEEDS_MORE_EVIDENCE: { tone: 'neutral', label: 'Needs evidence' },
  IRRELEVANT_ACTIVITY: { tone: 'neutral', label: 'Irrelevant activity' },
  KNOWN_INFRASTRUCTURE: { tone: 'neutral', label: 'Infrastructure' },
  AMBIGUOUS_ATTRIBUTION: { tone: 'warning', label: 'Ambiguous attribution' },
  UNSUPPORTED_ACCOUNT_ROLE: { tone: 'neutral', label: 'Unsupported role' },
  EXCLUDED_BY_POLICY: { tone: 'neutral', label: 'Excluded' },
};

const QUALIFICATION: Record<string, StateLabel> = {
  INSUFFICIENT_ACTIVITY: { tone: 'neutral', label: 'Insufficient activity' },
  INSUFFICIENT_HISTORY: { tone: 'neutral', label: 'Insufficient history' },
  INSUFFICIENT_FINANCIAL_EVIDENCE: { tone: 'neutral', label: 'Insufficient outcomes' },
  PRELIMINARY_POSITIVE_RETURN: { tone: 'neutral', label: 'Preliminary positive return' },
  PRELIMINARY_NEGATIVE_RETURN: { tone: 'neutral', label: 'Preliminary negative return' },
  RECENT_ACTIVITY_QUALIFIED: { tone: 'neutral', label: 'Recent activity qualified' },
  READY_FOR_DEEPER_ANALYSIS: { tone: 'info', label: 'Ready for deeper analysis' },
};

/** Coverage is never shown as complete unless the backend says so. */
const COVERAGE: Record<string, StateLabel> = {
  NOT_INVESTIGATED: { tone: 'neutral', label: 'Not investigated' },
  INVESTIGATION_PENDING: { tone: 'info', label: 'Pending' },
  PARTIAL_HISTORY: { tone: 'warning', label: 'Partial history' },
  BOUNDED_SAMPLE_COMPLETE: { tone: 'neutral', label: 'Bounded sample' },
  REQUESTED_SCOPE_COMPLETE: { tone: 'positive', label: 'Requested scope complete' },
  PROVIDER_HISTORY_UNAVAILABLE: { tone: 'warning', label: 'Provider history unavailable' },
  DECODER_COVERAGE_LIMITED: { tone: 'warning', label: 'Decoder coverage limited' },
  UNRESOLVED_GAPS: { tone: 'warning', label: 'Unresolved gaps' },
  UNKNOWN: { tone: 'warning', label: 'Unknown' },
};

const MONITORING: Record<string, StateLabel> = {
  LIVE_SUBSCRIPTION: { tone: 'info', label: 'Live subscription' },
  PERIODIC_HISTORY_REFRESH: { tone: 'info', label: 'Periodic refresh' },
  PASSIVE_LOCAL_OBSERVATION: { tone: 'neutral', label: 'Passive' },
  DEFERRED: { tone: 'warning', label: 'Deferred' },
  INACTIVE: { tone: 'neutral', label: 'Inactive' },
};

const JOB: Record<string, StateLabel> = {
  PENDING: { tone: 'neutral', label: 'Pending' },
  RUNNING: { tone: 'info', label: 'Running' },
  WAITING_DATA: { tone: 'info', label: 'Waiting for data' },
  DEFERRED: { tone: 'warning', label: 'Deferred' },
  COMPLETED: { tone: 'positive', label: 'Completed' },
  FAILED: { tone: 'negative', label: 'Failed' },
  CANCELLED: { tone: 'neutral', label: 'Cancelled' },
};

const WORK: Record<string, string> = {
  TRANSACTION_SAMPLING: 'Pool sample',
  INITIAL_HISTORY: 'Stage A history',
  EXPANDED_HISTORY: 'Stage B history',
  EXTENDED_HISTORY: 'Stage C history',
  ACTIVE_MONITORING: 'Monitoring refresh',
  DORMANT_RECHECK: 'Dormancy recheck',
};

const CYCLE: Record<string, StateLabel> = {
  RUNNING: { tone: 'info', label: 'Running' },
  COMPLETED: { tone: 'positive', label: 'Completed' },
  PARTIAL: { tone: 'warning', label: 'Partial' },
  DEFERRED: { tone: 'warning', label: 'Deferred' },
  FAILED: { tone: 'negative', label: 'Failed' },
};

const SOURCE_RUN: Record<string, StateLabel> = {
  OK: { tone: 'positive', label: 'OK' },
  EMPTY: { tone: 'neutral', label: 'Empty' },
  DISABLED: { tone: 'neutral', label: 'Disabled' },
  BUDGET_DEFERRED: { tone: 'warning', label: 'Budget deferred' },
  PROVIDER_COOLDOWN: { tone: 'warning', label: 'Provider cooldown' },
  FAILED: { tone: 'negative', label: 'Failed' },
};

const SELECTION: Record<string, StateLabel> = {
  CANDIDATE: { tone: 'neutral', label: 'Candidate' },
  SELECTED: { tone: 'info', label: 'Selected' },
  DEFERRED: { tone: 'neutral', label: 'Deferred' },
  EXCLUDED: { tone: 'neutral', label: 'Excluded' },
  INACTIVE: { tone: 'neutral', label: 'Inactive' },
};

const POOL: Record<string, StateLabel> = {
  UNVERIFIED: { tone: 'warning', label: 'Unverified' },
  SUPPORTED: { tone: 'positive', label: 'Supported' },
  UNSUPPORTED_PROGRAM: { tone: 'neutral', label: 'Unsupported program' },
  ACCOUNT_NOT_FOUND: { tone: 'warning', label: 'Account not found' },
};

/** Budget admission: provider work is paused, reduced or allowed. */
const BUDGET_MODE: Record<string, StateLabel> = {
  FULL: { tone: 'positive', label: 'Full' },
  CONSERVATION: { tone: 'warning', label: 'Conservation' },
  PAUSED: { tone: 'warning', label: 'Paused' },
};

const NAMES: Record<string, string> = {
  JUPITER_TOP_TRADED: 'Jupiter · top traded',
  JUPITER_TOP_ORGANIC: 'Jupiter · top organic',
  JUPITER_RECENT: 'Jupiter · recent',
  DEXSCREENER_BOOSTS: 'DEX Screener · boosts',
  LOCAL_ACTIVITY: 'Local activity',
  HISTORICAL: 'Historical',
  EXPLORATION: 'Exploration',
  ESTABLISHED: 'Established',
  EMERGING: 'Emerging',
  WALLET_HISTORY: 'Wallet history',
  MONITORING: 'Monitoring',
  DATA_ENGINE: 'Data engine',
  OPERATOR: 'Operator',
  MEME_CANDIDATE: 'Meme candidate',
  GENERAL_SPECULATIVE_TOKEN: 'Speculative token',
  ESTABLISHED_ASSET: 'Established asset',
  STABLECOIN: 'Stablecoin',
  WRAPPED_NATIVE_ASSET: 'Wrapped native',
  LIQUID_STAKING_ASSET: 'Liquid staking',
  LIQUIDITY_POSITION_TOKEN: 'LP token',
  KNOWN_INFRASTRUCTURE_ASSET: 'Infrastructure asset',
  UNKNOWN: 'Unknown',
};

export const lifecycleLabel = (v: string) => lookup(LIFECYCLE, v);
export const eligibilityLabel = (v: string) => lookup(ELIGIBILITY, v);
export const qualificationLabel = (v: string) => lookup(QUALIFICATION, v);
export const radarCoverageLabel = (v: string) => lookup(COVERAGE, v);
export const monitoringLabel = (v: string) => lookup(MONITORING, v);
export const radarJobLabel = (v: string) => lookup(JOB, v);
export const cycleLabel = (v: string) => lookup(CYCLE, v);
export const sourceRunLabel = (v: string) => lookup(SOURCE_RUN, v);
export const selectionLabel = (v: string) => lookup(SELECTION, v);
export const poolLabel = (v: string) => lookup(POOL, v);
export const budgetModeLabel = (v: string) => lookup(BUDGET_MODE, v);
export const workName = (v: string) => WORK[v] ?? v.replaceAll('_', ' ').toLowerCase();
/** Source, bucket and classification names. */
export const radarName = (v: string) => NAMES[v] ?? v.replaceAll('_', ' ').toLowerCase();

/** Lifecycle order for the registry breakdown and filter, so it never reshuffles. */
export const LIFECYCLE_ORDER = [
  'DISCOVERED',
  'SCREENING',
  'HISTORY_QUEUED',
  'INVESTIGATING',
  'PRELIMINARY_QUALIFIED',
  'PRIORITIZED',
  'MONITORED',
  'INSUFFICIENT_EVIDENCE',
  'DORMANT',
  'REJECTED',
  'ARCHIVED',
] as const;

/**
 * 0–100 priority decimal string at one fixed decimal (truncated toward zero,
 * so a score is never shown higher than computed). `null` → "—".
 */
export function formatScore(v: string | null): string {
  if (v === null) return '—';
  const m = /^(\d{1,3})(?:\.(\d+))?$/.exec(v);
  if (!m) return '—';
  const [, int = '0', frac = ''] = m;
  return `${int}.${(frac + '0').slice(0, 1)}`;
}

/** 0..1 component decimal string as a 0–100 figure with one decimal. */
export function formatComponent(v: string): string {
  const m = /^(0|1)(?:\.(\d+))?$/.exec(v);
  if (!m) return '—';
  const [, int = '0', frac = ''] = m;
  if (int === '1') return '100.0';
  const digits = (frac + '000').slice(0, 3);
  return `${String(Number(digits.slice(0, 2)))}.${digits.slice(2, 3)}`;
}
