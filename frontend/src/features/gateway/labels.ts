/**
 * Display vocabulary for GATEWAY states. Every state maps to a tone *and* a
 * text label; the tone is never the only signal.
 */
import type { Tone } from '@/components/ui/StatusIndicator.tsx';
import type {
  BudgetStateDto,
  CircuitStateDto,
  ContinuityDto,
  GatewayHealthState,
  PriorityDto,
  StreamStateDto,
} from '@/types/api.ts';

export interface StateLabel {
  tone: Tone;
  label: string;
}

export const healthLabel: Record<GatewayHealthState, StateLabel> = {
  DISABLED: { tone: 'neutral', label: 'Disabled' },
  UNKNOWN: { tone: 'neutral', label: 'Not measured' },
  HEALTHY: { tone: 'positive', label: 'Healthy' },
  DEGRADED: { tone: 'warning', label: 'Degraded' },
  UNAVAILABLE: { tone: 'negative', label: 'Unavailable' },
  BUDGET_EXHAUSTED: { tone: 'negative', label: 'Budget exhausted' },
};

export const circuitLabel: Record<CircuitStateDto, StateLabel> = {
  CLOSED: { tone: 'neutral', label: 'Closed' },
  OPEN: { tone: 'negative', label: 'Open' },
  HALF_OPEN: { tone: 'warning', label: 'Half-open' },
};

export const budgetLabel: Record<BudgetStateDto, StateLabel> = {
  NORMAL: { tone: 'positive', label: 'Normal' },
  CONSERVATION: { tone: 'warning', label: 'Conservation' },
  CRITICAL: { tone: 'warning', label: 'Critical' },
  NORMAL_BUDGET_EXHAUSTED: { tone: 'negative', label: 'Normal budget exhausted' },
  HARD_LIMIT_REACHED: { tone: 'negative', label: 'Hard limit reached' },
  ACCOUNT_USAGE_UNKNOWN: { tone: 'warning', label: 'Usage unknown' },
};

export const streamLabel: Record<StreamStateDto, StateLabel> = {
  DISCONNECTED: { tone: 'neutral', label: 'Disconnected' },
  CONNECTING: { tone: 'neutral', label: 'Connecting' },
  SUBSCRIBING: { tone: 'neutral', label: 'Subscribing' },
  CONNECTED: { tone: 'positive', label: 'Connected' },
  DEGRADED: { tone: 'warning', label: 'Degraded' },
  RECONNECTING: { tone: 'warning', label: 'Reconnecting' },
  DISABLED: { tone: 'neutral', label: 'Disabled' },
  BUDGET_EXHAUSTED: { tone: 'negative', label: 'Budget exhausted' },
  SHUTTING_DOWN: { tone: 'neutral', label: 'Shutting down' },
};

export const continuityLabel: Record<ContinuityDto, StateLabel> = {
  CONTINUOUS: { tone: 'positive', label: 'Continuous' },
  RECOVERED: { tone: 'info', label: 'Recovered' },
  GAP: { tone: 'warning', label: 'Gap' },
  UNKNOWN: { tone: 'warning', label: 'Unknown' },
};

/** "P0–P3" for the admitted priority range; `null` admits nothing. */
export function admittedRange(upTo: PriorityDto | null): string {
  return upTo === null ? 'None' : upTo === 'P0' ? 'P0 only' : `P0–${upTo}`;
}
