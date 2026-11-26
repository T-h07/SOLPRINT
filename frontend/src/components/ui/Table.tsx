import {
  createContext,
  type HTMLAttributes,
  type ReactNode,
  type TdHTMLAttributes,
  type ThHTMLAttributes,
  useContext,
} from 'react';

import { cn } from '@/lib/cn.ts';

/**
 * Data table primitives.
 *
 * - Density: `compact` (32 px rows) for high-frequency trading data,
 *   `standard` (40 px) where rows are interaction targets.
 * - `numeric` cells are right-aligned with tabular numerals so digits line
 *   up and values never shift columns as they update.
 * - The wrapper is a focusable, labelled region with contained horizontal
 *   scroll, so the page itself never scrolls sideways (WCAG 1.4.10
 *   two-dimensional-data exception) and keyboard users can scroll it.
 */
export type Density = 'compact' | 'standard';

const DensityContext = createContext<Density>('compact');

export function Table({
  label,
  density = 'compact',
  caption,
  children,
  className,
}: {
  /** Accessible name of the scroll region and table. */
  label: string;
  density?: Density;
  caption?: ReactNode;
  children: ReactNode;
  className?: string | undefined;
}) {
  return (
    <DensityContext.Provider value={density}>
      <div
        role="region"
        aria-label={label}
        tabIndex={0}
        className={cn('w-full overflow-x-auto', className)}
      >
        <table className="w-full border-collapse text-left text-sm">
          {caption ? <caption className="sr-only">{caption}</caption> : null}
          {children}
        </table>
      </div>
    </DensityContext.Provider>
  );
}

export function THead({ children }: { children: ReactNode }) {
  return <thead className="sticky top-0 z-[1] bg-raised">{children}</thead>;
}

export function TBody({ children }: { children: ReactNode }) {
  return <tbody>{children}</tbody>;
}

export function Tr({
  children,
  selected = false,
  className,
  ...rest
}: { children: ReactNode; selected?: boolean } & HTMLAttributes<HTMLTableRowElement>) {
  return (
    <tr
      aria-selected={selected || undefined}
      className={cn(
        'border-b border-line-subtle last:border-b-0 hover:bg-hover',
        // Selection: surface + leading bar, distinct from the focus ring.
        selected && 'bg-selected shadow-[inset_2px_0_0_var(--color-fg)]',
        className,
      )}
      {...rest}
    >
      {children}
    </tr>
  );
}

export function Th({
  children,
  numeric = false,
  className,
  ...rest
}: { children?: ReactNode; numeric?: boolean } & ThHTMLAttributes<HTMLTableCellElement>) {
  return (
    <th
      scope="col"
      className={cn(
        'h-8 border-b border-line px-3 text-2xs font-medium tracking-wider whitespace-nowrap text-fg-muted uppercase',
        numeric ? 'text-right' : 'text-left',
        className,
      )}
      {...rest}
    >
      {children}
    </th>
  );
}

export function Td({
  children,
  numeric = false,
  className,
  ...rest
}: { children?: ReactNode; numeric?: boolean } & TdHTMLAttributes<HTMLTableCellElement>) {
  const density = useContext(DensityContext);
  return (
    <td
      className={cn(
        'px-3 whitespace-nowrap',
        density === 'compact' ? 'h-8 text-xs' : 'h-10 text-sm',
        numeric ? 'num text-right' : 'text-left',
        className,
      )}
      {...rest}
    >
      {children}
    </td>
  );
}
