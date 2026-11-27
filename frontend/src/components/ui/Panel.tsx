import { type HTMLAttributes, type ReactNode, useId } from 'react';

import { cn } from '@/lib/cn.ts';

/**
 * Workspace pane: a bordered region of the grid, not a floating card. No
 * shadow, small radius, header separated by a rule. `flush` removes body
 * padding for tables that run edge to edge.
 */
export function Panel({
  title,
  meta,
  actions,
  flush = false,
  className,
  children,
  ...rest
}: {
  title: string;
  /** Right-aligned secondary information (data age, counts). */
  meta?: ReactNode;
  actions?: ReactNode;
  flush?: boolean;
  className?: string | undefined;
  children: ReactNode;
} & Omit<HTMLAttributes<HTMLElement>, 'title'>) {
  const headingId = useId();
  return (
    <section
      aria-labelledby={headingId}
      className={cn('flex min-w-0 flex-col rounded-md border border-line bg-base', className)}
      {...rest}
    >
      <header className="flex min-h-10 items-center gap-3 border-b border-line px-3">
        <h2 id={headingId} className="text-md font-medium text-fg">
          {title}
        </h2>
        <div className="ml-auto flex items-center gap-3 text-xs text-fg-muted">
          {meta}
          {actions}
        </div>
      </header>
      <div className={cn('min-w-0 flex-1', flush ? '' : 'p-3')}>{children}</div>
    </section>
  );
}

export function Separator({
  orientation = 'horizontal',
  className,
}: {
  orientation?: 'horizontal' | 'vertical';
  className?: string | undefined;
}) {
  return (
    <div
      role="separator"
      aria-orientation={orientation}
      className={cn(
        'shrink-0 bg-line',
        orientation === 'horizontal' ? 'h-px w-full' : 'h-full w-px self-stretch',
        className,
      )}
    />
  );
}

/** Label/value rows for a pane: aligned grid, not a stack of cards. */
export function DefinitionList({
  items,
  className,
}: {
  /** `wrap` lets a long value break across lines instead of truncating. */
  items: { term: string; value: ReactNode; wrap?: boolean }[];
  className?: string | undefined;
}) {
  return (
    <dl
      className={cn(
        'grid grid-cols-[minmax(7rem,max-content)_1fr] gap-x-6 gap-y-2 text-sm',
        className,
      )}
    >
      {items.map((item) => (
        <div key={item.term} className="contents">
          <dt className="text-fg-muted">{item.term}</dt>
          <dd className={cn('m-0 min-w-0 text-fg', item.wrap ? 'break-words' : 'truncate')}>
            {item.value}
          </dd>
        </div>
      ))}
    </dl>
  );
}
