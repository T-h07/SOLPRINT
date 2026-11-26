import { X } from 'lucide-react';
import { Dialog } from 'radix-ui';
import type { ReactNode } from 'react';

import { IconButton } from './Button.tsx';

/**
 * Side inspector (Radix Dialog). Keeps list context visible behind it;
 * focus is trapped while open and returned to the trigger on close; Esc
 * closes (non-destructive).
 */
export function Drawer({
  open,
  onOpenChange,
  title,
  description,
  children,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  title: string;
  description?: string;
  children: ReactNode;
}) {
  return (
    <Dialog.Root open={open} onOpenChange={onOpenChange}>
      <Dialog.Portal>
        <Dialog.Overlay className="fixed inset-0 z-[var(--z-overlay)] bg-canvas/70" />
        <Dialog.Content
          className="fixed top-0 right-0 z-[var(--z-overlay)] flex h-full w-full max-w-md flex-col border-l border-line bg-base shadow-overlay"
          {...(description ? {} : { 'aria-describedby': undefined })}
        >
          <header className="flex h-11 items-center gap-3 border-b border-line px-3">
            <Dialog.Title className="text-md font-medium">{title}</Dialog.Title>
            <Dialog.Close asChild>
              <IconButton
                label="Close inspector"
                icon={<X className="size-4" />}
                className="ml-auto"
              />
            </Dialog.Close>
          </header>
          {description ? (
            <Dialog.Description className="border-b border-line px-3 py-2 text-xs text-fg-muted">
              {description}
            </Dialog.Description>
          ) : null}
          <div className="flex-1 overflow-y-auto p-3">{children}</div>
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  );
}
