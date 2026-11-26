import { Component, type ErrorInfo, type ReactNode } from 'react';
import { isRouteErrorResponse, useRouteError } from 'react-router';

import { Button } from '@/components/ui/Button.tsx';
import { DataState } from '@/components/ui/DataState.tsx';

function Fallback({ detail }: { detail: string }) {
  return (
    <div className="flex min-h-full items-start justify-center bg-canvas p-6">
      <div className="w-full max-w-xl rounded-md border border-line bg-base p-4">
        <DataState
          kind="error"
          title="The interface hit an unexpected error."
          detail={`${detail} Trading state is held by the backend and is unaffected by this view.`}
          action={
            <Button
              size="sm"
              onClick={() => {
                window.location.reload();
              }}
            >
              Reload
            </Button>
          }
        />
      </div>
    </div>
  );
}

/** Last-resort boundary around the whole app. */
export class AppErrorBoundary extends Component<{ children: ReactNode }, { failed: boolean }> {
  override state = { failed: false };

  static getDerivedStateFromError(): { failed: boolean } {
    return { failed: true };
  }

  override componentDidCatch(error: Error, info: ErrorInfo): void {
    console.error('SOLPRINT UI error', error, info.componentStack);
  }

  override render(): ReactNode {
    return this.state.failed ? <Fallback detail="Rendering failed." /> : this.props.children;
  }
}

/** Route-level boundary (rendered by the router on loader/render errors). */
export function RouteError() {
  const error = useRouteError();
  const detail = isRouteErrorResponse(error)
    ? `Route error ${error.status.toString()}.`
    : 'Rendering failed.';
  return <Fallback detail={detail} />;
}
