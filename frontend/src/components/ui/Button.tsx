import { cva, type VariantProps } from 'class-variance-authority';
import { type ButtonHTMLAttributes, forwardRef, type ReactNode } from 'react';

import { cn } from '@/lib/cn.ts';

import { Tooltip } from './Tooltip.tsx';

/**
 * Buttons. Every variant defines default, hover, pressed (:active),
 * selected (aria-pressed / data-selected), focus-visible (global ring) and
 * disabled states. Destructive actions carry text, never only an icon.
 */
const buttonVariants = cva(
  [
    'inline-flex shrink-0 items-center justify-center gap-2 whitespace-nowrap select-none',
    'rounded-md border font-medium transition-ui',
    'disabled:pointer-events-none disabled:border-line disabled:bg-base disabled:text-fg-disabled',
  ],
  {
    variants: {
      variant: {
        primary:
          'border-fg bg-fg text-canvas hover:border-fg-secondary hover:bg-fg-secondary active:bg-fg-muted',
        secondary: [
          'border-line-control bg-raised text-fg hover:bg-hover active:bg-pressed',
          'aria-pressed:border-fg aria-pressed:bg-selected data-[selected=true]:border-fg',
        ],
        ghost: [
          'border-transparent bg-transparent text-fg-secondary hover:bg-hover hover:text-fg active:bg-pressed',
          'aria-pressed:bg-selected aria-pressed:text-fg',
        ],
        destructive:
          'border-negative bg-negative-surface text-negative hover:bg-negative hover:text-canvas active:opacity-90',
      },
      size: {
        sm: 'h-7 px-2.5 text-xs',
        md: 'h-8 px-3 text-sm',
        lg: 'h-10 px-4 text-sm',
      },
    },
    defaultVariants: { variant: 'secondary', size: 'md' },
  },
);

export interface ButtonProps
  extends ButtonHTMLAttributes<HTMLButtonElement>, VariantProps<typeof buttonVariants> {}

export const Button = forwardRef<HTMLButtonElement, ButtonProps>(function Button(
  { className, variant, size, type = 'button', ...props },
  ref,
) {
  return (
    <button
      ref={ref}
      type={type}
      className={cn(buttonVariants({ variant, size }), className)}
      {...props}
    />
  );
});

export interface IconButtonProps extends Omit<
  ButtonHTMLAttributes<HTMLButtonElement>,
  'children' | 'aria-label'
> {
  /** Required accessible name; also shown as tooltip. */
  label: string;
  icon: ReactNode;
  variant?: 'secondary' | 'ghost';
  size?: 'sm' | 'md';
}

/** Square icon-only button. Always labelled; never used for irreversible actions. */
export const IconButton = forwardRef<HTMLButtonElement, IconButtonProps>(function IconButton(
  { label, icon, variant = 'ghost', size = 'md', className, type = 'button', ...props },
  ref,
) {
  return (
    <Tooltip content={label}>
      <button
        ref={ref}
        type={type}
        aria-label={label}
        className={cn(
          buttonVariants({ variant, size }),
          size === 'sm' ? 'w-7 px-0' : 'w-8 px-0',
          className,
        )}
        {...props}
      >
        <span aria-hidden="true" className="flex">
          {icon}
        </span>
      </button>
    </Tooltip>
  );
});
