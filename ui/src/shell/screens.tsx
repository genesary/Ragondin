// The six screens, widest to narrowest (the front-end design, § 3). Runs,
// Compare and Replay are built (src/runs/, src/compare/, src/replay/) — Replay
// loaded as its own chunk, since it carries the canvas; each other screen is
// its empty state here: one sentence
// on the default path and the action that leads on. A screen's own issue
// replaces its empty state with its content and keeps the route shape
// src/routes.ts gives it.
import { lazy, Suspense, useEffect, useRef, type RefObject } from 'react';
import { ButtonLink, EmptyState, Sheet } from '../../design/index.ts';
import type { ApiClient } from '../api/client.ts';
import { CompareScreen } from '../compare/CompareScreen.tsx';
import { formatHash, type Route, type ScreenName } from '../routes.ts';
import { RunsScreen } from '../runs/RunsScreen.tsx';
import { Loading } from './states.tsx';

// The canvas and its two libraries come with Replay, in a chunk of their own,
// so the shell and the other screens do not pay for them.
const ReplayScreen = lazy(() => import('../replay/ReplayScreen.tsx').then((m) => ({ default: m.ReplayScreen })));

export const SCREENS: readonly { screen: ScreenName; label: string; bare: Route }[] = [
  { screen: 'runs', label: 'Runs', bare: { screen: 'runs' } },
  { screen: 'pipeline', label: 'Pipeline', bare: { screen: 'pipeline' } },
  { screen: 'compare', label: 'Compare', bare: { screen: 'compare', ids: [] } },
  { screen: 'replay', label: 'Replay', bare: { screen: 'replay' } },
  { screen: 'editor', label: 'Editor', bare: { screen: 'editor' } },
  { screen: 'setup', label: 'Setup', bare: { screen: 'setup' } },
];

type Action = { label: string; to: Route };
type Empty = { heading: string; sentence: string; action?: Action };

const openRuns: Action = { label: 'Open Runs', to: { screen: 'runs' } };
const openCompare: Action = { label: 'Open Compare', to: { screen: 'compare', ids: [] } };

/** What a screen shows before it has data, given the state its route carries. */
function emptyOf(route: Exclude<Route, { screen: 'runs' | 'compare' }>): Empty {
  switch (route.screen) {
    case 'pipeline':
      return route.name === undefined
        ? { heading: 'No pipeline chosen', sentence: 'Choose a pipeline in Runs to see each of its nodes against every benchmark it ran on.', action: openRuns }
        : { heading: `Nothing to show for ${route.name} yet`, sentence: `Each node of ${route.name} against every benchmark it ran on appears here.`, action: openRuns };
    case 'replay':
      return { heading: 'No query chosen', sentence: 'Open a run from Runs, or a query from Compare, to follow it through the pipeline, node by node.', action: openCompare };
    case 'editor':
      return route.name === undefined
        ? { heading: 'No pipeline open', sentence: 'Open a pipeline from Runs to edit it on the canvas.', action: openRuns }
        : { heading: `Nothing to show for ${route.name} yet`, sentence: `The nodes and edges of ${route.name} appear here, on the canvas.`, action: openRuns };
    case 'setup':
      return { heading: 'No benchmarks or services shown yet', sentence: 'The benchmarks this workspace holds and the services it binds are set up here.' };
  }
}

/**
 * Moves focus to `heading` whenever `address` changes from the one the page
 * loaded at, so a screen reader announces the new view and the keyboard starts
 * from it. The load itself keeps the browser's own focus: a deep link opens
 * where the browser puts it. The skip compares addresses rather than counting
 * renders, because StrictMode runs a mount's effects twice.
 */
export function useFocusOnChange(heading: RefObject<HTMLElement | null>, address: string) {
  const loadedAt = useRef(address);
  const moved = useRef(false);
  useEffect(() => {
    if (!moved.current && address === loadedAt.current) return;
    moved.current = true;
    heading.current?.focus();
  }, [heading, address]);
}

/** The screen a route shows: its name as the page's heading, then its state. */
export function Screen({ route, heading, client }: { route: Route; heading: RefObject<HTMLHeadingElement | null>; client: ApiClient }) {
  const label = SCREENS.find((s) => s.screen === route.screen)?.label ?? route.screen;
  const title = (
    <h1 ref={heading} tabIndex={-1} className="rg-visually-hidden">
      {label}
    </h1>
  );
  if (route.screen === 'runs') {
    return (
      <>
        {title}
        <RunsScreen client={client} sel={route.sel ?? []} />
      </>
    );
  }
  if (route.screen === 'compare') {
    return (
      <>
        {title}
        <CompareScreen client={client} ids={route.ids} baseline={route.baseline} />
      </>
    );
  }
  if (route.screen === 'replay' && 'run' in route) {
    return (
      <>
        {title}
        <Suspense fallback={<Loading label="Opening Replay" />}>
          <ReplayScreen client={client} run={route.run} query={route.query} with={'with' in route ? route.with : undefined} />
        </Suspense>
      </>
    );
  }
  const empty = emptyOf(route);
  return (
    <>
      {title}
      <Sheet>
        <EmptyState
          heading={empty.heading}
          action={
            empty.action === undefined ? undefined : (
              <ButtonLink kind="primary" size="l" href={formatHash(empty.action.to)}>
                {empty.action.label}
              </ButtonLink>
            )
          }
        >
          {empty.sentence}
        </EmptyState>
      </Sheet>
    </>
  );
}
