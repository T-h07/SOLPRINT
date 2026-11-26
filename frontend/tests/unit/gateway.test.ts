import { describe, expect, it } from 'vitest';

import { admittedRange, budgetLabel, healthLabel } from '@/features/gateway/labels.ts';
import { isGatewayBudgets, isGatewayStatus, isGatewayUsage } from '@/lib/api/guards.ts';
import { formatBytes, formatShare } from '@/lib/format/number.ts';

describe('gateway formatting', () => {
  it('formats byte counts in decimal units', () => {
    expect(formatBytes(0)).toBe('0 B');
    expect(formatBytes(999)).toBe('999 B');
    expect(formatBytes(12_400)).toBe('12.4 KB');
    expect(formatBytes(48_211_904)).toBe('48.21 MB');
    expect(formatBytes(-1)).toBe('—');
    expect(formatBytes(null)).toBe('—');
  });

  it('rounds shares down so a budget never looks exhausted early', () => {
    expect(formatShare(999_999, 1_000_000)).toBe('99.9%');
    expect(formatShare(1_000_000, 1_000_000)).toBe('100.0%');
    expect(formatShare(212_904, 1_000_000)).toBe('21.2%');
    expect(formatShare(1, 0)).toBe('—');
  });

  it('labels every state with text, not only tone', () => {
    for (const l of [...Object.values(healthLabel), ...Object.values(budgetLabel)]) {
      expect(l.label.length).toBeGreaterThan(0);
    }
    expect(healthLabel.UNKNOWN.label).toBe('Not measured');
    expect(admittedRange('P3')).toBe('P0–P3');
    expect(admittedRange('P0')).toBe('P0 only');
    expect(admittedRange(null)).toBe('None');
  });
});

describe('gateway guards', () => {
  it('reject non-contract bodies', () => {
    expect(isGatewayStatus({})).toBe(false);
    expect(isGatewayStatus('<html>Bad Gateway</html>')).toBe(false);
    expect(isGatewayBudgets({ generated_at: 'x', budgets: [{ credits_used: '1' }] })).toBe(false);
    expect(isGatewayUsage({ since: 'x', process: [], history: [] })).toBe(false);
  });

  it('refuse a status that claims execution capability', () => {
    expect(
      isGatewayStatus({
        enabled: true,
        execution_capability: 'SIGN',
        generated_at: 'x',
        providers: [],
        streams: {},
        persistence: {},
        scheduler: {},
        cache: {},
        budget: null,
      }),
    ).toBe(false);
  });
});
