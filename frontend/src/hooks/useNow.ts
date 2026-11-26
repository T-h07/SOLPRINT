import { useEffect, useState } from 'react';

/**
 * Current wall-clock time, refreshed every `intervalMs`. Used only for
 * display (ages, clock). Each consumer re-renders on its own tick, so a
 * ticking clock never re-renders unrelated regions.
 */
export function useNow(intervalMs = 1_000): number {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    const id = window.setInterval(() => {
      setNow(Date.now());
    }, intervalMs);
    return () => {
      window.clearInterval(id);
    };
  }, [intervalMs]);
  return now;
}
