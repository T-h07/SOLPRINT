import type { ReactNode } from 'react';

import { useDocumentTitle } from '@/hooks/useDocumentTitle.ts';

/** Workspace page frame: compact title row, then the pane grid. */
export function PageLayout({
  title,
  context,
  meta,
  children,
}: {
  title: string;
  /** Short qualifier after the title, e.g. "Foundation · Phase 00". */
  context?: string;
  /** Right-aligned page-level metadata (data age). */
  meta?: ReactNode;
  children: ReactNode;
}) {
  useDocumentTitle(title);
  return (
    <div className="flex flex-col gap-3 p-3 lg:p-4">
      <div className="flex min-h-8 flex-wrap items-baseline gap-x-3 gap-y-1">
        <h1 className="text-lg font-medium text-fg">{title}</h1>
        {context ? <span className="text-xs text-fg-muted">{context}</span> : null}
        {meta ? <div className="ml-auto">{meta}</div> : null}
      </div>
      {children}
    </div>
  );
}
