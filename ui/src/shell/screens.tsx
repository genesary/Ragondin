// The six screens, widest to narrowest (the front-end design, § 3). Runs is
// built (src/runs/); each other screen is its empty state here: one sentence
// on the default path and the action that leads on. A screen's own issue
// replaces its empty state with its content and keeps the route shape
// src/routes.ts gives it.
import { useEffect, useRef, type RefObject } from 'react';
import { ButtonLink, EmptyState, Sheet } from '../../design/index.ts';
import type { ApiClient } from '../api/client.ts';
import { formatHash, type Route, type ScreenName } from '../routes.ts';
import { RunsScreen } from '../runs/RunsScreen.tsx';

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
const count = (n: number, one: string) => `${n.toLocaleString('en-US')} ${one}${n === 1 ? '' : 's'}`;

/** What a screen shows before it has data, given the state its route carries. */
function emptyOf(route: Exclude<Route, { screen: 'runs' }>): Empty {
  switch (route.screen) {
    case 'pipeline':
      return route.name === undefined
        ? { heading: 'No pipeline chosen', sentence: 'Choose a pipeline in Runs to see each of its nodes against every benchmark it ran on.', action: openRuns }
        : { heading: `Nothing to show for ${route.name} yet`, sentence: `Each node of ${route.name} against every benchmark it ran on appears here.`, action: openRuns };
    case 'compare':
      return route.ids.length === 0
        ? { heading: 'No runs chosen to compare', sentence: 'Choose a baseline and up to four runs in Runs to compare them stage by stage.', action: openRuns }
        : {
            heading: `Nothing to show for ${count(route.ids.length, 'run')} yet`,
            sentence: 'The runs chosen, stage by stage against their baseline, appear here.',
            action: openRuns,
          };
    case 'replay':
      if ('run' in route && route.query === undefined) {
        return { heading: `No query chosen for run ${route.run}`, sentence: 'A query of this run, node by node through the pipeline, appears here.', action: openCompare };
      }
      return 'run' in route
        ? { heading: `Nothing to show for query ${route.query} yet`, sentence: 'This query, node by node through the pipeline, appears here.', action: openCompare }
        : { heading: 'No query chosen', sentence: 'Open a query from Compare to follow it through the pipeline, node by node.', action: openCompare };
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
