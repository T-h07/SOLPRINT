import { describe, expect, it } from 'vitest';

import {
  formatBaseUnits,
  formatBpsAsPercent,
  formatDecimalString,
  formatInteger,
  formatLamportsAsSol,
  formatLatencyUs,
  MINUS,
} from '@/lib/format/number.ts';
import { formatAge, formatDuration, formatUtc, formatUtcClock } from '@/lib/format/time.ts';

describe('number formatting', () => {
  it('groups integers and uses U+2212 for negatives', () => {
    expect(formatInteger(1234567)).toBe('1,234,567');
    expect(formatInteger(-42000)).toBe(`${MINUS}42,000`);
    expect(formatInteger(Number.MAX_SAFE_INTEGER + 2)).toBe('—');
    expect(formatInteger(18_446_744_073_709_551_615n)).toBe('18,446,744,073,709,551,615');
  });

  it('formats u64 base units exactly, without floating point', () => {
    // u64::MAX lamports cannot be represented by a JS number.
    expect(formatLamportsAsSol('18446744073709551615', 9)).toBe('18,446,744,073.709551615');
    expect(formatLamportsAsSol('1', 9)).toBe('0.000000001');
    expect(formatLamportsAsSol('1500000000')).toBe('1.5000');
    expect(formatBaseUnits('0', 6, 2)).toBe('0.00');
  });

  it('rounds down by default and half-up on request', () => {
    expect(formatBaseUnits('19999', 4, 2)).toBe('1.99');
    expect(formatBaseUnits('19950', 4, 2, 'half-up')).toBe('2.00');
    expect(formatBaseUnits('19949', 4, 2, 'half-up')).toBe('1.99');
  });

  it('rejects non-canonical or invalid base-unit input', () => {
    for (const bad of ['', '-1', '01', '1.5', '1e9', ' 1', 'abc']) {
      expect(formatBaseUnits(bad, 9, 4)).toBe('—');
    }
    expect(formatBaseUnits('1', -1, 2)).toBe('—');
  });

  it('rounds decimal strings half-up with stable width', () => {
    expect(formatDecimalString('0.000012345', 8)).toBe('0.00001235');
    expect(formatDecimalString('-1.005', 2)).toBe(`${MINUS}1.01`);
    expect(formatDecimalString('-0.0001', 2)).toBe('0.00');
    expect(formatDecimalString('1234.5', 2)).toBe('1,234.50');
    expect(formatDecimalString('NaN', 2)).toBe('—');
  });

  it('signs percentages so direction is readable without colour', () => {
    expect(formatBpsAsPercent(125)).toBe('+1.25%');
    expect(formatBpsAsPercent(-40)).toBe(`${MINUS}0.40%`);
    expect(formatBpsAsPercent(0)).toBe('0.00%');
    expect(formatBpsAsPercent(1.5)).toBe('—');
  });

  it('formats latency with explicit units', () => {
    expect(formatLatencyUs(412)).toBe('412 µs');
    expect(formatLatencyUs(12_400)).toBe('12.4 ms');
    expect(formatLatencyUs(1_240_000)).toBe('1.24 s');
    expect(formatLatencyUs(null)).toBe('—');
    expect(formatLatencyUs(-1)).toBe('—');
  });
});

describe('time formatting', () => {
  const t = Date.parse('2026-11-26T09:04:05.678Z');

  it('formats absolute times in UTC with an explicit suffix', () => {
    expect(formatUtc(t)).toBe('2026-11-26 09:04:05 UTC');
    expect(formatUtc('not a date')).toBe('—');
    expect(formatUtcClock(t)).toBe('09:04:05');
  });

  it('formats durations compactly', () => {
    expect(formatDuration(45)).toBe('45s');
    expect(formatDuration(134)).toBe('2m 14s');
    expect(formatDuration(3 * 3600 + 5 * 60)).toBe('3h 05m');
    expect(formatDuration(4 * 86400 + 2 * 3600)).toBe('4d 02h');
    expect(formatDuration(-1)).toBe('—');
  });

  it('formats age relative to a supplied clock', () => {
    expect(formatAge(t, t + 500)).toBe('just now');
    expect(formatAge(t, t + 12_000)).toBe('12s ago');
    // Clock skew never produces a negative age.
    expect(formatAge(t + 5_000, t)).toBe('just now');
  });
});
