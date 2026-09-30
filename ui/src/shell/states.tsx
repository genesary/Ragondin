// The four states every screen shows (the front-end design, § 3): empty —
// design/'s EmptyState, one sentence and the action — in progress, error and
// loaded, wired to the outcome of a request.
import type { ReactNode } from 'react';
import { Button, InlineMessage } from '../../design/index.ts';
import type { ApiProblem } from '../api/client.ts';

/** A request's outcome as a screen renders it. */
export type RequestState<T> = { status: 'loading' } | { status: 'error'; problem: ApiProblem } | { status: 'loaded'; value: T };

/**
 * A request in flight, said in words. Never a bare spinner: a request has no
 * count to show, so its label is the whole of the state.
 */
export function Loading({ label }: { label: string }) {
  return (
    <span role="status" aria-busy="true">
      {label}
    </span>
  );
}

/** Where a validation failure is, in the words `ragondin validate` uses. */
function where(location: ApiProblem['location']): string | null {
  if (location === null) return null;
  if (location.edge !== null && location.edge !== undefined) {
    return `At the edge from ${location.edge.from} to ${location.edge.to}, input ${location.edge.port}.`;
  }
  return location.node === null ? null : `At node ${location.node}.`;
}

/**
 * A failed request, inline and next to what it concerns: what happened, how
 * to fix it, and the stable code — never a modal, and never dismissed by
 * itself.
 */
export function ErrorState({ problem, onRetry }: { problem: ApiProblem; onRetry?: () => void }) {
  const at = where(problem.location);
  return (
    <InlineMessage
      tone="critical"
      title={problem.message}
      action={
        onRetry === undefined ? undefined : (
          <Button size="s" onClick={onRetry}>
            Retry
          </Button>
        )
      }
    >
      {at === null ? null : <>{at} </>}
      {problem.hint} <code>{problem.code}</code>
    </InlineMessage>
  );
}

/** One request's three live states: in flight, failed, loaded. */
export function Resource<T>({
  state,
  loading,
  error,
  children,
}: {
  state: RequestState<T>;
  /** What is in flight, in words. */
  loading: string;
  /** How a failure renders here; the inline error state by default. */
  error?: (problem: ApiProblem) => ReactNode;
  children: (value: T) => ReactNode;
}) {
  switch (state.status) {
    case 'loading':
      return <Loading label={loading} />;
    case 'error':
      return <>{error === undefined ? <ErrorState problem={state.problem} /> : error(state.problem)}</>;
    case 'loaded':
      return <>{children(state.value)}</>;
  }
}
