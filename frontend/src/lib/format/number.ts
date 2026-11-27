/**
 * Financial and operational number formatting.
 *
 * Rules (see docs/architecture/DATA_CONVENTIONS.md):
 * - Amounts arrive as integer base-unit strings and are formatted with
 *   BigInt arithmetic. They are never converted to a JS `number`, which
 *   cannot represent u64 values exactly.
 * - Decimal prices arrive as decimal strings and are rounded as strings.
 * - Precision is fixed per call, so a column never changes width as values
 *   update.
 * - Negative values use U+2212 MINUS SIGN; positive deltas carry an explicit
 *   "+" so sign is readable without colour.
 */

export const MINUS = '−';
const GROUP = /\B(?=(\d{3})+(?!\d))/g;

function group(intDigits: string): string {
  return intDigits.replace(GROUP, ',');
}

export function formatInteger(value: number | bigint): string {
  if (typeof value === 'number' && !Number.isSafeInteger(value)) return '—';
  const s = value.toString();
  return s.startsWith('-') ? MINUS + group(s.slice(1)) : group(s);
}

export type RoundingMode = 'down' | 'half-up';

/**
 * Format an unsigned integer base-unit string (e.g. lamports, token raw
 * amount) with `decimals` implied decimals, displayed with exactly
 * `places` fractional digits.
 */
export function formatBaseUnits(
  raw: string,
  decimals: number,
  places: number,
  rounding: RoundingMode = 'down',
): string {
  if (!/^(0|[1-9]\d*)$/.test(raw)) return '—';
  if (!Number.isInteger(decimals) || decimals < 0 || !Number.isInteger(places) || places < 0) {
    return '—';
  }
  let value = BigInt(raw);
  let scale = decimals;
  if (places < scale) {
    const divisor = 10n ** BigInt(scale - places);
    const quotient = value / divisor;
    const remainder = value % divisor;
    value = rounding === 'half-up' && remainder * 2n >= divisor ? quotient + 1n : quotient;
    scale = places;
  }
  const padded = value.toString().padStart(scale + 1, '0');
  const intPart = padded.slice(0, padded.length - scale);
  const fracPart = (scale > 0 ? padded.slice(padded.length - scale) : '').padEnd(places, '0');
  return places > 0 ? `${group(intPart)}.${fracPart}` : group(intPart);
}

export const LAMPORTS_DECIMALS = 9;

export function formatLamportsAsSol(lamports: string, places = 4): string {
  return formatBaseUnits(lamports, LAMPORTS_DECIMALS, places);
}

/** Round a decimal string (e.g. "0.000012345") to fixed places, half-up. */
export function formatDecimalString(value: string, places: number): string {
  const m = /^(-?)(\d+)(?:\.(\d+))?$/.exec(value);
  if (!m) return '—';
  const [, sign = '', int = '0', frac = ''] = m;
  const digits = int + frac;
  const formatted = formatBaseUnits(BigInt(digits).toString(), frac.length, places, 'half-up');
  const isZero = /^[0.,]+$/.test(formatted);
  return sign && !isZero ? MINUS + formatted : formatted;
}

/** Signed percentage from basis points: 125 -> "+1.25%", -40 -> "−0.40%". */
export function formatBpsAsPercent(bps: number, places = 2): string {
  if (!Number.isInteger(bps)) return '—';
  const abs = formatBaseUnits(Math.abs(bps).toString(), 2, places, 'half-up');
  if (bps > 0) return `+${abs}%`;
  if (bps < 0) return `${MINUS}${abs}%`;
  return `${abs}%`;
}

/** Latency in microseconds -> "640 µs" / "12.4 ms" / "1.24 s". */
export function formatLatencyUs(us: number | null | undefined): string {
  if (us === null || us === undefined || !Number.isFinite(us) || us < 0) return '—';
  if (us < 1_000) return `${Math.round(us).toString()} µs`;
  if (us < 1_000_000) return `${(us / 1_000).toFixed(1)} ms`;
  return `${(us / 1_000_000).toFixed(2)} s`;
}

/** Byte counts -> "512 B" / "12.4 KB" / "3.21 MB" / "1.05 GB" (decimal units, as metered). */
export function formatBytes(bytes: number | null | undefined): string {
  if (bytes === null || bytes === undefined || !Number.isSafeInteger(bytes) || bytes < 0)
    return '—';
  if (bytes < 1_000) return `${bytes.toString()} B`;
  if (bytes < 1_000_000) return `${(bytes / 1_000).toFixed(1)} KB`;
  if (bytes < 1_000_000_000) return `${(bytes / 1_000_000).toFixed(2)} MB`;
  return `${(bytes / 1_000_000_000).toFixed(2)} GB`;
}

/**
 * Integer share `part / whole` as a percentage with fixed places, rounded
 * down so a nearly exhausted budget never displays as fully exhausted early.
 */
export function formatShare(part: number, whole: number, places = 1): string {
  if (!Number.isSafeInteger(part) || !Number.isSafeInteger(whole) || whole <= 0 || part < 0) {
    return '—';
  }
  const scale = 10n ** BigInt(places);
  const scaled = (BigInt(part) * 100n * scale) / BigInt(whole);
  return `${formatBaseUnits(scaled.toString(), places, places)}%`;
}

/**
 * Raw token amount with its mint's decimals, at most `maxPlaces` fractional
 * digits (rounded down, never up). Unknown decimals show the raw integer
 * with an explicit "raw" suffix instead of guessing a scale.
 */
export function formatTokenAmount(
  raw: string | null,
  decimals: number | null,
  maxPlaces = 4,
): string {
  if (raw === null) return '—';
  if (decimals === null) return /^(0|[1-9]\d*)$/.test(raw) ? `${group(raw)} raw` : '—';
  return formatBaseUnits(raw, decimals, Math.min(decimals, maxPlaces));
}

/** Signed base-unit delta ("-500000000" → "−0.5000", "+…" for gains). */
export function formatSignedBaseUnits(
  raw: string | null,
  decimals: number | null,
  maxPlaces = 4,
): string {
  if (raw === null) return '—';
  const m = /^(-?)(0|[1-9]\d*)$/.exec(raw);
  if (!m) return '—';
  const [, sign = '', abs = '0'] = m;
  const v = formatTokenAmount(abs, decimals, maxPlaces);
  if (abs === '0') return v;
  return sign ? `${MINUS}${v}` : `+${v}`;
}

/** "So11…1112": first and last four characters of a base58 identifier. */
export function shortId(id: string, keep = 4): string {
  return id.length <= keep * 2 + 1 ? id : `${id.slice(0, keep)}…${id.slice(-keep)}`;
}
