import { Tooltip as RadixTooltip } from 'radix-ui';
import type { ReactNode } from 'react';

/**
 * Tooltip (Radix primitive, SOLPRINT styling). Supplementary only: anything
 * a tooltip says must also be reachable without hover (label, table cell or
 * detail view). Opens on keyboard focus as well as hover.
 */
export function Tooltip({
  content,
  children,
  side = 'top',
}: {
  content: ReactNode;
  children: ReactNode;
  side?: 'top' | 'right' | 'bottom' | 'left';
}) {
  return (
    <RadixTooltip.Root>
      <RadixTooltip.Trigger asChild>{children}</RadixTooltip.Trigger>
      <RadixTooltip.Portal>
        <RadixTooltip.Content
          side={side}
          sideOffset={6}
          collisionPadding={8}
          className="z-[var(--z-tooltip)] max-w-72 rounded-sm bg-raised px-2 py-1 text-xs text-fg shadow-overlay"
        >
          {content}
        </RadixTooltip.Content>
      </RadixTooltip.Portal>
    </RadixTooltip.Root>
  );
}

export const TooltipProvider = RadixTooltip.Provider;
