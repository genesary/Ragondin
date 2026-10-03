import { useCallback, useEffect, useRef, useState } from 'react';
import { ButtonLink, InlineMessage, TopBar } from '../design/index.ts';
import type { ApiClient, ApiProblem } from './api/client.ts';
import { openEvents, type ConnectionState } from './api/events.ts';
import type { Workspace } from './api/types.ts';
import { pipelineEntry } from './pipeline/last.ts';
import { formatHash, useRoute, viewOf } from './routes.ts';
import { judgeBuild } from './shell/build.ts';
import { Screen, SCREENS, useFocusOnChange } from './shell/screens.tsx';
import './shell/Shell.css';
import { ErrorState, type RequestState } from './shell/states.tsx';
import { ThemeControl } from './shell/ThemeControl.tsx';
import { WorkspaceIndicator } from './shell/WorkspaceIndicator.tsx';

export type AppProps = {
  client: ApiClient;
  /** This bundle's build identity (src/shell/build.ts). */
  build: string;
  /** Reloads the page, fetching the UI of the build that now answers. */
  reload: () => void;
  /** The event stream to follow, under the API's base address; none is opened without it. */
  eventsPath?: string;
};

/** What the top bar shows of the stream, in words beside the dot (the front-end design, § 8). */
const CONNECTION: Record<ConnectionState, { label: string; connected: boolean }> = {
  connecting: { label: 'connecting', connected: false },
  connected: { label: 'connected', connected: true },
  disconnected: { label: 'disconnected — retrying', connected: false },
};

const mismatch = (served: string | null, expected: string): ApiProblem => ({
  code: 'build_mismatch',
  message: `This page is build ${expected}, and the server answering is ${served === null ? 'a build that gives no identity' : `build ${served}`}.`,
  hint: 'The page was reloaded once and a different build still answers. Restart `ragondin ui` from the build you mean to use, then reload.',
  location: null,
  status: null,
});

/**
 * The shell: the top bar — the name, the workspace, the six screens, the
 * connection state and the theme — and the screen the address shows. It
 * reads the workspace once on load, and again whenever the event stream
 * reconnects, comparing each time the build that answered with its own.
 */
export function App({ client, build, reload, eventsPath }: AppProps) {
  const route = useRoute();
  const [workspace, setWorkspace] = useState<RequestState<Workspace>>({ status: 'loading' });
  const [refused, setRefused] = useState<ApiProblem | null>(null);
  const [connection, setConnection] = useState<ConnectionState | null>(null);
  const heading = useRef<HTMLHeadingElement>(null);
  // Keyed on the view, not the whole address: state within a view written to
  // the address (a selection, a correction in place) moves no focus.
  useFocusOnChange(heading, route === null ? window.location.hash : viewOf(route));

  // Kept in a ref: a new function from a re-rendering parent is not a reason
  // to read the workspace again or to reopen the stream.
  const reloadRef = useRef(reload);
  useEffect(() => {
    reloadRef.current = reload;
  }, [reload]);

  const readWorkspace = useCallback(async () => {
    const result = await client.get('/workspace');
    // A request that got no answer carries no identity to compare.
    if (result.ok || result.problem.code !== 'network_failed' || result.problem.status !== null) {
      const judgement = judgeBuild(result.build, build);
      if (judgement.verdict === 'reload') {
        reloadRef.current();
        return;
      }
      if (judgement.verdict === 'different') {
        setRefused(mismatch(judgement.served, judgement.expected));
        return;
      }
    }
    setWorkspace(result.ok ? { status: 'loaded', value: result.value } : { status: 'error', problem: result.problem });
  }, [client, build]);

  useEffect(() => {
    void readWorkspace();
  }, [readWorkspace]);

  useEffect(() => {
    if (eventsPath === undefined) return;
    const stream = openEvents(eventsPath, { onState: setConnection, onReconnect: () => void readWorkspace() });
    return () => stream.close();
  }, [eventsPath, readWorkspace]);

  const retry = () => {
    setWorkspace({ status: 'loading' });
    void readWorkspace();
  };

  return (
    <>
      <TopBar
        workspace={<WorkspaceIndicator state={workspace} />}
        links={SCREENS.map((s) => ({ label: s.label, href: formatHash(s.screen === 'pipeline' ? pipelineEntry() : s.bare), current: route?.screen === s.screen }))}
        services={[]}
        {...(connection === null ? {} : { status: CONNECTION[connection] })}
        end={<ThemeControl />}
      />
      <main className="rg-shell__main">
        {refused !== null ? (
          <ErrorState problem={refused} />
        ) : (
          <>
            {workspace.status === 'error' ? <ErrorState problem={workspace.problem} onRetry={retry} /> : null}
            {route === null ? (
              <>
                <h1 ref={heading} tabIndex={-1} className="rg-visually-hidden">
                  No such screen
                </h1>
                <InlineMessage
                  tone="critical"
                  title={`No screen at ${window.location.hash}.`}
                  action={
                    <ButtonLink size="s" href={formatHash({ screen: 'runs' })}>
                      Open Runs
                    </ButtonLink>
                  }
                >
                  The address names no screen of this build. Check the link, or start from Runs.
                </InlineMessage>
              </>
            ) : (
              <Screen route={route} heading={heading} client={client} />
            )}
          </>
        )}
      </main>
    </>
  );
}
