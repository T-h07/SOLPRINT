import { type ClassValue, clsx } from 'clsx';
import { extendTailwindMerge } from 'tailwind-merge';

// Teach tailwind-merge about SOLPRINT's custom type scale so `text-sm` and
// `text-fg-muted` are not treated as conflicting.
const twMerge = extendTailwindMerge({
  extend: {
    theme: {
      text: ['2xs', 'xs', 'sm', 'base', 'md', 'lg', 'xl'],
    },
  },
});

export function cn(...inputs: ClassValue[]): string {
  return twMerge(clsx(inputs));
}
