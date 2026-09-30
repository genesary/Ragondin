// The six screens, widest to narrowest (the front-end design, § 3). Each is
// its empty state here: one sentence on the default path and the action that
// leads on. A screen's own issue replaces its empty state with its content and
// keeps the route shape src/routes.ts gives it.
import { Button, EmptyState, Sheet } from '../../design/index.ts';
import { navigate, type Route, type ScreenName } from '../routes.ts';

export const SCREENS: readonly { screen: ScreenName; label: string; bare: Route }[] = [
  { screen: 'runs', label: 'Runs', bare: { screen: 'runs' } },
  { screen: 'pipeline', label: 'Pipeline', bare: { screen: 'pipeline' } },
  { screen: 'compare', label: 'Compare', bare: { screen: 'compare', ids: [] } },
  { screen: 'replay', label: 'Replay', bare: { screen: 'replay' } },
  { screen: 'editor', label: 'Editor', bare: { screen: 'editor' } },
  { screen: 'setup', label: 'Setup', bare: { screen: 'setup' } },
];

type Empty = { heading: string; sentence: string; action?: { label: string; to: Route } };

const openRuns = { label: 'Open Runs', to: { screen: 'runs' } } as const;

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
            heading: `Nothing to show for ${route.ids.length} runs yet`,
            sentence: 'The runs chosen, stage by stage against their baseline, appear here.',
            action: openRuns,
          };
    case 'replay':
      return 'run' in route
        ? { heading: `Nothing to show for query ${route.query} yet`, sentence: 'This query, node by node through the pipeline, appears here.', action: { label: 'Open Compare', to: { screen: 'compare', ids: [] } } }
        : {
            heading: 'No query chosen',
            sentence: 'Open a query from Compare to follow it through the pipeline, node by node.',
            action: { label: 'Open Compare', to: { screen: 'compare', ids: [] } },
          };
    case 'editor':
      return route.name === undefined
        ? { heading: 'No pipeline open', sentence: 'Open a pipeline from Runs to edit it on the canvas.', action: openRuns }
        : { heading: `Nothing to show for ${route.name} yet`, sentence: `The nodes and edges of ${route.name} appear here, on the canvas.`, action: openRuns };
    case 'setup':
      return { heading: 'No benchmarks or services shown yet', sentence: 'The benchmarks this workspace holds and the services it binds are set up here.' };
  }
}

/** The screen a route shows: its name as the page's heading, then its state. */
export function Screen({ route }: { route: Route }) {
  const label = SCREENS.find((s) => s.screen === route.screen)?.label ?? route.screen;
  const empty = emptyOf(route);
  return (
    <>
      <h1 className="rg-visually-hidden">{label}</h1>
      <Sheet>
        <EmptyState
          heading={empty.heading}
          action={
            empty.action === undefined ? undefined : (
              <Button kind="primary" size="l" onClick={() => empty.action !== undefined && navigate(empty.action.to)}>
                {empty.action.label}
              </Button>
            )
          }
        >
          {empty.sentence}
        </EmptyState>
      </Sheet>
    </>
  );
}
