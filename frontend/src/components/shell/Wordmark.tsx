/** SOLPRINT wordmark: three bars + letter-spaced name. Restrained, monochrome. */
export function Wordmark() {
  return (
    <span className="inline-flex items-center gap-2.5 text-fg">
      <svg width="14" height="14" viewBox="0 0 14 14" aria-hidden="true">
        <rect x="0" y="0" width="14" height="3" fill="currentColor" />
        <rect x="0" y="5.5" width="14" height="3" className="fill-fg-secondary" />
        <rect x="0" y="11" width="8" height="3" fill="currentColor" />
      </svg>
      <span className="text-sm font-semibold tracking-[0.2em]">SOLPRINT</span>
    </span>
  );
}
