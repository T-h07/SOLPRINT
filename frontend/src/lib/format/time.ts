/**
 * Time formatting. All absolute times are shown in UTC with an explicit
 * "UTC" suffix; relative ages are computed from a supplied `now` so callers
 * (and tests) control the clock.
 */

const pad = (n: number, width = 2) => n.toString().padStart(width, '0');

/** "2026-11-26 14:03:22 UTC" */
export function formatUtc(input: string | number | Date): string {
  const d = new Date(input);
  if (Number.isNaN(d.getTime())) return '—';
  return (
    `${d.getUTCFullYear().toString()}-${pad(d.getUTCMonth() + 1)}-${pad(d.getUTCDate())} ` +
    `${pad(d.getUTCHours())}:${pad(d.getUTCMinutes())}:${pad(d.getUTCSeconds())} UTC`
  );
}

/** "14:03:22" (UTC) */
export function formatUtcClock(input: number | Date): string {
  const d = new Date(input);
  return `${pad(d.getUTCHours())}:${pad(d.getUTCMinutes())}:${pad(d.getUTCSeconds())}`;
}

/** Compact duration: "45s", "2m 14s", "3h 05m", "4d 02h". */
export function formatDuration(totalSeconds: number): string {
  if (!Number.isFinite(totalSeconds) || totalSeconds < 0) return '—';
  const s = Math.floor(totalSeconds);
  const days = Math.floor(s / 86_400);
  const hours = Math.floor((s % 86_400) / 3_600);
  const minutes = Math.floor((s % 3_600) / 60);
  const seconds = s % 60;
  if (days > 0) return `${days.toString()}d ${pad(hours)}h`;
  if (hours > 0) return `${hours.toString()}h ${pad(minutes)}m`;
  if (minutes > 0) return `${minutes.toString()}m ${pad(seconds)}s`;
  return `${seconds.toString()}s`;
}

/** Age of a timestamp relative to `now`: "just now", "12s ago", "2m 14s ago". */
export function formatAge(then: number, now: number): string {
  const seconds = Math.max(0, Math.floor((now - then) / 1_000));
  if (seconds < 2) return 'just now';
  return `${formatDuration(seconds)} ago`;
}
