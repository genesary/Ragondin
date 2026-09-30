// The six screens, widest to narrowest (the front-end design, § 3). Each is
// its empty state here: one sentence on the default path and the action that
// leads on. A screen's own issue replaces its empty state with its content and
// keeps the route shape src/routes.ts gives it.
import { useEffect, useRef, type RefObject } from 'react';
import { ButtonLink, EmptyState, Sheet } from '../../design/index.ts';
import { formatHash, type Route, type ScreenName } from '../routes.ts';

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
function emptyOf(route: Route): Empty {
  switch (route.screen) {
    case 'runs':
      return {
        heading: 'No runs to show yet',
        sentence: 'Runs are listed here once one is launched. A first run starts in Setup, with a benchmark to download.',
        action: { label: 'Open Setup', to: { screen: 'setup' } },
      };
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
 * Moves focus to `heading` whenever `address` changes after the first render,
 * so a screen reader announces the new view and the keyboard starts from it.
 * The first render keeps the browser's own focus: a deep link opens where
 * the browser puts it.
 */
export function useFocusOnChange(heading: RefObject<HTMLElement | null>, address: string) {
  const first = useRef(true);
  useEffect(() => {
    if (first.current) {
      first.current = false;
      return;
    }
    heading.current?.focus();
  }, [heading, address]);
}

/** The screen a route shows: its name as the page's heading, then its state. */
export function Screen({ route, heading }: { route: Route; heading: RefObject<HTMLHeadingElement | null> }) {
  const label = SCREENS.find((s) => s.screen === route.screen)?.label ?? route.screen;
  const empty = emptyOf(route);
  return (
    <>
      <h1 ref={heading} tabIndex={-1} className="rg-visually-hidden">
        {label}
      </h1>
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
