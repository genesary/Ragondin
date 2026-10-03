// The Editor screen: the canvas in write mode over a pipeline document held
// in state (src/editor/Editor.tsx). ARCHITECTURE.md § The editor.
import { lazy, Suspense, useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { Button, ButtonLink, EmptyState, Sheet } from '../../design/index.ts';
import type { ApiClient, ApiResult } from '../api/client.ts';
import type { PipelineDetail, ServiceListing, Workspace } from '../api/types.ts';
import { formatHash } from '../routes.ts';
import { ErrorState, Loading, Resource, type RequestState } from '../shell/states.tsx';
import { emptyDocument } from './document.ts';
import { grammarOf } from './ports.ts';

// The canvas and its two libraries come with the editor, in a chunk of their
// own, so the shell and this screen's empty state do not pay for them.
const Editor = lazy(() => import('./Editor.tsx').then((m) => ({ default: m.Editor })));

export type EditorScreenProps = {
  client: ApiClient;
  /** The pipeline the address names, if any. */
  name: string | undefined;
  /** The shell's read of `GET /workspace`: its capabilities fill the palette. */
  workspace: RequestState<Workspace>;
};

/** One read on mount and on Retry, only the last one asked landing. */
function useOnce<T>(read: ((signal: AbortSignal) => Promise<ApiResult<T>>) | null): [RequestState<T>, () => void] {
  const [state, setState] = useState<RequestState<T>>({ status: 'loading' });
  const reader = useRef(read);
  reader.current = read;
  const latest = useRef<AbortController | null>(null);
  const ask = useCallback(() => {
    latest.current?.abort();
    const read = reader.current;
    if (read === null) return;
    const controller = new AbortController();
    latest.current = controller;
    setState({ status: 'loading' });
    void read(controller.signal).then((result) => {
      if (controller.signal.aborted) return;
      setState(result.ok ? { status: 'loaded', value: result.value } : { status: 'error', problem: result.problem });
    });
  }, []);
  const active = read !== null;
  useEffect(() => {
    if (active) ask();
    return () => latest.current?.abort();
  }, [active, ask]);
  return [state, ask];
}

/** The editor over a new, empty document, once the services are read. */
function NewPipeline({ client, workspace }: { client: ApiClient; workspace: Workspace }) {
  const [services, retry] = useOnce<ServiceListing>((signal) => client.get('/services', { signal }));
  const [selected, setSelected] = useState<string | null>(null);
  const [initial] = useState(emptyDocument);
  const grammar = useMemo(() => grammarOf(workspace.capabilities), [workspace.capabilities]);
  return (
    <Resource state={services} loading="Reading the services Setup bound" error={(problem) => <ErrorState problem={problem} onRetry={retry} />}>
      {(listing) => (
        <Suspense fallback={<Loading label="Opening the editor" />}>
        <Editor
          client={client}
          title="New pipeline"
          initial={initial}
          capabilities={workspace.capabilities}
          services={listing.services}
          grammar={grammar}
          selected={selected}
          onSelect={setSelected}
        />
        </Suspense>
      )}
    </Resource>
  );
}

/** A stored pipeline: read, and said to be out of the canvas's reach until the API serves it as a graph. */
function Stored({ client, name }: { client: ApiClient; name: string }) {
  const [detail, retry] = useOnce<PipelineDetail>((signal) => client.get('/pipelines/{name}', { name }, { signal }));
  return (
    <Resource state={detail} loading={`Reading ${name}`} error={(problem) => <ErrorState problem={problem} onRetry={retry} />}>
      {() => (
        <Sheet>
          <EmptyState
            heading={`${name} cannot be opened on the canvas yet`}
            action={
              <ButtonLink kind="primary" size="l" href={formatHash({ screen: 'editor' })}>
                Start a new pipeline
              </ButtonLink>
            }
          >
            The API serves a pipeline as its text, and the editor holds a pipeline as its wire-schema document, which it does not parse from text. Until the API serves a stored pipeline's graph, the canvas edits new pipelines only.
          </EmptyState>
        </Sheet>
      )}
    </Resource>
  );
}

/**
 * `#editor`: one action, a new pipeline, and a link to Runs. `#editor/<name>`:
 * the stored pipeline, read, and why the canvas cannot open it yet.
 */
export function EditorScreen({ client, name, workspace }: EditorScreenProps) {
  const [started, setStarted] = useState(false);
  if (name !== undefined) return <Stored client={client} name={name} />;
  if (started) {
    if (workspace.status === 'loading') return <Loading label="Reading this build's capabilities" />;
    // A failed workspace read is the shell's to show, with Retry, above the screen.
    if (workspace.status === 'error') return null;
    return <NewPipeline client={client} workspace={workspace.value} />;
  }
  return (
    <Sheet>
      <EmptyState
        heading="No pipeline open"
        action={
          <>
            <Button kind="primary" size="l" onClick={() => setStarted(true)}>
              Start a new pipeline
            </Button>
            <ButtonLink kind="quiet" size="l" href={formatHash({ screen: 'runs' })}>
              Open Runs
            </ButtonLink>
          </>
        }
      >
        Start a new pipeline on the canvas, from what this build can run.
      </EmptyState>
    </Sheet>
  );
}
