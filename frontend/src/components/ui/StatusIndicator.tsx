import { cn } from '@/lib/cn.ts';

export type Tone = 'positive' | 'negative' | 'warning' | 'info' | 'neutral';

const toneText: Record<Tone, string> = {
  positive: 'text-positive',
  negative: 'text-negative',
  warning: 'text-warning',
  info: 'text-info',
  neutral: 'text-fg-secondary',
};

/**
 * Shape encodes the tone independently of colour, so the state survives
 * colour-blindness, greyscale screenshots and high-contrast modes:
 *   positive ● filled circle   warning ▲ triangle   negative ■ square
 *   info ◆ diamond             neutral ○ hollow circle
 */
function Glyph({ tone }: { tone: Tone }) {
  const common = { width: 8, height: 8, viewBox: '0 0 8 8', 'aria-hidden': true as const };
  switch (tone) {
    case 'positive':
      return (
        <svg {...common}>
          <circle cx="4" cy="4" r="3.5" fill="currentColor" />
        </svg>
      );
    case 'warning':
      return (
        <svg {...common}>
          <path d="M4 0.5 7.6 7.5H0.4Z" fill="currentColor" />
        </svg>
      );
    case 'negative':
      return (
        <svg {...common}>
          <rect x="0.5" y="0.5" width="7" height="7" fill="currentColor" />
        </svg>
      );
    case 'info':
      return (
        <svg {...common}>
          <path d="M4 0 8 4 4 8 0 4Z" fill="currentColor" />
        </svg>
      );
    case 'neutral':
      return (
        <svg {...common}>
          <circle cx="4" cy="4" r="3" fill="none" stroke="currentColor" strokeWidth="1.25" />
        </svg>
      );
  }
}

/**
 * Status = glyph + text label. The label is mandatory: a coloured dot on
 * its own is never a status.
 */
export function StatusIndicator({
  tone,
  label,
  detail,
  className,
}: {
  tone: Tone;
  label: string;
  detail?: string | undefined;
  className?: string | undefined;
}) {
  return (
    <span className={cn('inline-flex items-center gap-1.5 whitespace-nowrap', className)}>
      <span className={cn('flex shrink-0', toneText[tone])}>
        <Glyph tone={tone} />
      </span>
      <span className={cn('text-xs font-medium tracking-wide uppercase', toneText[tone])}>
        {label}
      </span>
      {detail ? <span className="num text-xs text-fg-muted">{detail}</span> : null}
    </span>
  );
}
