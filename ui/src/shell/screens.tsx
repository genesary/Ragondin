// The six screens, widest to narrowest (the front-end design, § 3). Runs,
// Pipeline, Compare, Replay, Editor and Setup are built (src/runs/,
// src/pipeline/, src/compare/, src/replay/, src/editor/, src/setup/) — Replay
// and the editor's canvas loaded as chunks of their own, since they carry the
// canvas; Replay before a run is chosen is its run picker, which carries no
// canvas and so is imported here directly. A screen's own issue
// replaces its empty state with its content and keeps the route shape
// src/routes.ts gives it.
import { lazy, Suspense, useEffect, useRef, type RefObject } from 'react';
import type { ApiClient } from '../api/client.ts';
import type { Workspace } from '../api/types.ts';
import { CompareScreen } from '../compare/CompareScreen.tsx';
import { EditorScreen } from '../editor/EditorScreen.tsx';
import { PipelineScreen } from '../pipeline/PipelineScreen.tsx';
import type { Route, ScreenName } from '../routes.ts';
import { RunPicker } from '../replay/RunPicker.tsx';
import { RunsScreen } from '../runs/RunsScreen.tsx';
import { SetupScreen } from '../setup/SetupScreen.tsx';
import { Loading, type RequestState } from './states.tsx';

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

/** What the shell hands a screen besides its route: the client, and the workspace it read. */
export type ScreenProps = {
  route: Route;
  heading: RefObject<HTMLHeadingElement | null>;
  client: ApiClient;
  /** The shell's read of `GET /workspace`, the one the build identity handshake judged. */
  workspace: RequestState<Workspace>;
  /** Reads the workspace again, keeping the current read on screen. */
  refreshWorkspace: () => void;
  /** Reads the workspace again after a failure. */
  retryWorkspace: () => void;
};

/** The screen a route shows: its name as the page's heading, then its state. */
export function Screen({ route, heading, client, workspace, refreshWorkspace, retryWorkspace }: ScreenProps) {
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
        <RunsScreen client={client} sel={route.sel ?? []} job={route.job} launch={route.launch} bench={route.bench} store={workspace.status === 'loaded' ? workspace.value.path : null} counts={workspace.status === 'loaded' ? workspace.value.counts : null} refreshWorkspace={refreshWorkspace} />
      </>
    );
  }
  if (route.screen === 'pipeline') {
    return (
      <>
        {title}
        <PipelineScreen client={client} name={route.name} />
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
          <ReplayScreen client={client} run={route.run} query={route.query} node={'node' in route ? route.node : undefined} with={'with' in route ? route.with : undefined} set={'set' in route ? route.set : undefined} />
        </Suspense>
      </>
    );
  }
  if (route.screen === 'replay' && 'job' in route) {
    return (
      <>
        {title}
        <Suspense fallback={<Loading label="Opening Replay" />}>
          <ReplayScreen client={client} job={route.job} query={route.query} node={route.node} />
        </Suspense>
      </>
    );
  }
  if (route.screen === 'editor') {
    return (
      <>
        {title}
        <EditorScreen client={client} name={route.name} node={route.node} workspace={workspace} />
      </>
    );
  }
  if (route.screen === 'setup') {
    return (
      <>
        {title}
        <SetupScreen client={client} workspace={workspace} refreshWorkspace={refreshWorkspace} retryWorkspace={retryWorkspace} section={route.section} />
      </>
    );
  }
  return (
    <>
      {title}
      <RunPicker client={client} />
    </>
  );
}
