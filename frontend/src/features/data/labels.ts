/**
 * Display vocabulary for DATA ENGINE states (values defined by the backend,
 * see docs/architecture/DATA_ENGINE.md). Every state maps to a tone *and* a
 * text label; unknown values are rendered verbatim with a neutral tone, never
 * guessed into a known state.
 */
import type { Tone } from '@/components/ui/StatusIndicator.tsx';

export interface StateLabel {
  tone: Tone;
  label: string;
}

function lookup(table: Record<string, StateLabel>, value: string): StateLabel {
  return table[value] ?? { tone: 'neutral', label: value.replaceAll('_', ' ').toLowerCase() };
}

const DECODE: Record<string, StateLabel> = {
  VERIFIED: { tone: 'positive', label: 'Verified' },
  PARTIALLY_DECODED: { tone: 'warning', label: 'Partial' },
  AMBIGUOUS: { tone: 'warning', label: 'Ambiguous' },
  UNSUPPORTED: { tone: 'neutral', label: 'Unsupported' },
  INSUFFICIENT_EVIDENCE: { tone: 'warning', label: 'Insufficient evidence' },
  INVALID: { tone: 'negative', label: 'Invalid' },
};

/** Finality is never shown as final unless the backend says FINALIZED. */
const FINALITY: Record<string, StateLabel> = {
  OBSERVED_PROCESSED: { tone: 'neutral', label: 'Processed' },
  OBSERVED_CONFIRMED: { tone: 'info', label: 'Confirmed' },
  FINALIZED: { tone: 'positive', label: 'Finalized' },
  FINALITY_UNKNOWN: { tone: 'warning', label: 'Finality unknown' },
  POSSIBLY_ORPHANED: { tone: 'warning', label: 'Possibly orphaned' },
  ORPHANED: { tone: 'negative', label: 'Orphaned' },
};

const ATTRIBUTION: Record<string, StateLabel> = {
  VERIFIED_OWNER: { tone: 'positive', label: 'Owner' },
  VERIFIED_AUTHORITY: { tone: 'positive', label: 'Authority' },
  VERIFIED_BENEFICIARY: { tone: 'positive', label: 'Beneficiary' },
  MULTIPLE_POSSIBLE_ACTORS: { tone: 'warning', label: 'Multiple actors' },
  UNKNOWN_OWNER: { tone: 'warning', label: 'Unknown owner' },
  PROGRAM_CONTROLLED: { tone: 'neutral', label: 'Program' },
  AMBIGUOUS: { tone: 'warning', label: 'Ambiguous' },
};

const JOB: Record<string, StateLabel> = {
  PENDING: { tone: 'neutral', label: 'Pending' },
  FETCHING: { tone: 'info', label: 'Fetching' },
  FETCH_RETRY: { tone: 'warning', label: 'Retry' },
  COMPLETED: { tone: 'positive', label: 'Completed' },
  UNAVAILABLE: { tone: 'warning', label: 'Unavailable' },
  UNSUPPORTED: { tone: 'neutral', label: 'Unsupported' },
  QUARANTINED: { tone: 'negative', label: 'Quarantined' },
  CANCELLED: { tone: 'neutral', label: 'Cancelled' },
};

const SEVERITY: Record<string, StateLabel> = {
  INFO: { tone: 'info', label: 'Info' },
  WARNING: { tone: 'warning', label: 'Warning' },
  INVALID: { tone: 'negative', label: 'Invalid' },
  OPERATIONAL: { tone: 'warning', label: 'Operational' },
  CRITICAL: { tone: 'negative', label: 'Critical' },
};

const BACKFILL: Record<string, StateLabel> = {
  PENDING: { tone: 'neutral', label: 'Pending' },
  RUNNING: { tone: 'info', label: 'Running' },
  PAUSED: { tone: 'neutral', label: 'Paused' },
  PAUSED_BY_BUDGET: { tone: 'warning', label: 'Paused · budget' },
  PAUSED_BY_PROVIDER: { tone: 'warning', label: 'Paused · provider' },
  PAUSED_BY_QUEUE: { tone: 'warning', label: 'Paused · queue' },
  COMPLETED: { tone: 'positive', label: 'Completed' },
  COMPLETED_WITH_GAPS: { tone: 'warning', label: 'Completed with gaps' },
  FAILED: { tone: 'negative', label: 'Failed' },
  CANCELLED: { tone: 'neutral', label: 'Cancelled' },
};

const COVERAGE: Record<string, StateLabel> = {
  NONE: { tone: 'neutral', label: 'None' },
  LIVE_ONLY: { tone: 'info', label: 'Live only' },
  PARTIAL_HISTORY: { tone: 'warning', label: 'Partial history' },
  ADDRESS_HISTORY_EXHAUSTED: { tone: 'positive', label: 'History exhausted' },
  BOUNDARY_REACHED: { tone: 'info', label: 'Boundary reached' },
  GAPS_UNRESOLVED: { tone: 'warning', label: 'Gaps unresolved' },
};

export const decodeLabel = (v: string) => lookup(DECODE, v);
export const finalityLabel = (v: string) => lookup(FINALITY, v);
export const attributionLabel = (v: string) => lookup(ATTRIBUTION, v);
export const jobLabel = (v: string) => lookup(JOB, v);
export const severityLabel = (v: string) => lookup(SEVERITY, v);
export const backfillLabel = (v: string) => lookup(BACKFILL, v);
export const coverageLabel = (v: string) => lookup(COVERAGE, v);

/** Order used when listing job states, so columns do not reshuffle. */
export const JOB_ORDER = [
  'PENDING',
  'FETCHING',
  'FETCH_RETRY',
  'COMPLETED',
  'UNAVAILABLE',
  'UNSUPPORTED',
  'QUARANTINED',
  'CANCELLED',
] as const;

export const SEVERITY_ORDER = ['CRITICAL', 'INVALID', 'OPERATIONAL', 'WARNING', 'INFO'] as const;

const PROTOCOL_NAMES: Record<string, string> = {
  JUPITER: 'Jupiter',
  PUMP_FUN: 'Pump.fun',
  PUMPSWAP: 'PumpSwap',
  RAYDIUM_AMM_V4: 'Raydium AMM v4',
  RAYDIUM_CPMM: 'Raydium CPMM',
  RAYDIUM_CLMM: 'Raydium CLMM',
  ORCA_WHIRLPOOL: 'Orca Whirlpool',
};

export const protocolName = (p: string) => PROTOCOL_NAMES[p] ?? p;
export const PROTOCOLS = Object.keys(PROTOCOL_NAMES);

/** Upper-case enum to readable lower-case text ("BONDING_CURVE_TRADE" → "bonding curve trade"). */
export const humanize = (v: string) => v.replaceAll('_', ' ').toLowerCase();
