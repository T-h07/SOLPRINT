import { ChevronDown } from 'lucide-react';
import {
  forwardRef,
  type InputHTMLAttributes,
  type ReactNode,
  type SelectHTMLAttributes,
  useId,
} from 'react';

import { cn } from '@/lib/cn.ts';

const controlBase = [
  'h-8 w-full rounded-md border border-line-control bg-base px-2.5 text-sm text-fg transition-ui',
  'placeholder:text-fg-muted hover:border-fg-muted',
  'disabled:cursor-not-allowed disabled:border-line disabled:text-fg-disabled',
  'aria-invalid:border-negative',
].join(' ');

export const Input = forwardRef<HTMLInputElement, InputHTMLAttributes<HTMLInputElement>>(
  function Input({ className, ...props }, ref) {
    return <input ref={ref} className={cn(controlBase, className)} {...props} />;
  },
);

/** Native select: correct keyboard and screen-reader behaviour for free. */
export const Select = forwardRef<HTMLSelectElement, SelectHTMLAttributes<HTMLSelectElement>>(
  function Select({ className, children, ...props }, ref) {
    return (
      <div className="relative">
        <select
          ref={ref}
          className={cn(controlBase, 'cursor-pointer appearance-none pr-8', className)}
          {...props}
        >
          {children}
        </select>
        <ChevronDown
          aria-hidden="true"
          className="pointer-events-none absolute top-1/2 right-2.5 size-3.5 -translate-y-1/2 text-fg-muted"
        />
      </div>
    );
  },
);

/** Label + control + hint/error, wired with ids for assistive tech. */
export function Field({
  label,
  hint,
  error,
  children,
}: {
  label: string;
  hint?: string;
  error?: string;
  children: (props: {
    id: string;
    'aria-describedby'?: string;
    'aria-invalid'?: boolean;
  }) => ReactNode;
}) {
  const id = useId();
  const hintId = `${id}-hint`;
  const describedBy = error || hint ? hintId : undefined;
  return (
    <div className="flex flex-col gap-1.5">
      <label htmlFor={id} className="text-xs font-medium text-fg-secondary">
        {label}
      </label>
      {children({
        id,
        ...(describedBy ? { 'aria-describedby': describedBy } : {}),
        ...(error ? { 'aria-invalid': true } : {}),
      })}
      {error ? (
        <p id={hintId} className="text-xs text-negative">
          <span aria-hidden="true">✕ </span>
          {error}
        </p>
      ) : hint ? (
        <p id={hintId} className="text-xs text-fg-muted">
          {hint}
        </p>
      ) : null}
    </div>
  );
}
