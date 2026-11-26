import { useEffect } from 'react';

/** Consistent document titles: "System · SOLPRINT". */
export function useDocumentTitle(title: string): void {
  useEffect(() => {
    document.title = `${title} · SOLPRINT`;
  }, [title]);
}
