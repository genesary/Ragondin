// The Editor screen: the canvas in write mode over a pipeline document held
// in state (src/editor/Editor.tsx): a new one, or a stored one opened from its
// typed document. ARCHITECTURE.md § The editor.
import { lazy, Suspense, useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { Button, ButtonLink, EmptyState, Sheet } from '../../design/index.ts';
import type { ApiClient, ApiResult } from '../api/client.ts';
import type { PipelineDetail, ServiceListing, Workspace } from '../api/types.ts';
import { formatHash, navigate } from '../routes.ts';
import { ErrorState, Loading, Resource, type RequestState } from '../shell/states.tsx';
import { emptyDocument, type WireDocument } from './document.ts';
import { grammarOf } from './ports.ts';

// The canvas and its two libraries come with the editor, in a chunk of their
// own, so the shell and this screen's empty state do not pay for them.
const Editor = lazy(() => import('./Editor.tsx').then((m) => ({ default: m.Editor })));

export type EditorScreenProps = {
  client: ApiClient;
  /** The pipeline the address names, if any. */
  name: string | undefined;
  /** The node the address selects in it, if any. */
  node?: string | undefined;
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

/** The editor over `initial`, once the services are read. */
function Editing({ client, workspace, title, stored = null, initial, selected, onSelect }: { client: ApiClient; workspace: Workspace; title: string; stored?: string | null; initial: WireDocument; selected: string | null; onSelect: (id: string | null) => void }) {
  const [services, retry] = useOnce<ServiceListing>((signal) => client.get('/services', { signal }));
  const grammar = useMemo(() => grammarOf(workspace.capabilities), [workspace.capabilities]);
  return (
    <Resource state={services} loading="Reading the services Setup bound" error={(problem) => <ErrorState problem={problem} onRetry={retry} />}>
      {(listing) => (
        <Suspense fallback={<Loading label="Opening the editor" />}>
        <Editor
          client={client}
          title={title}
          stored={stored}
          initial={initial}
          capabilities={workspace.capabilities}
          services={listing.services}
          grammar={grammar}
          selected={selected}
          onSelect={onSelect}
        />
        </Suspense>
      )}
    </Resource>
  );
}

/** The editor over a new, empty document; its selection is the screen's, since no address names it. */
function NewPipeline({ client, workspace }: { client: ApiClient; workspace: Workspace }) {
  const [selected, setSelected] = useState<string | null>(null);
  const [initial] = useState(emptyDocument);
  return <Editing client={client} workspace={workspace} title="New pipeline" initial={initial} selected={selected} onSelect={setSelected} />;
}

/**
 * A stored pipeline, opened from the typed document `GET /pipelines/{name}`
 * serves — whether or not it validates (ADR-C40 § 4) — its selected node the
 * address's: restored from `#editor/<name>/node/<id>`, and written there.
 * A document whose text does not read into the wire schema, or holds a value
 * the typed document cannot carry, has none, and is said to be text only.
 */
function Stored({ client, name, node, workspace }: { client: ApiClient; name: string; node: string | undefined; workspace: RequestState<Workspace> }) {
  const [detail, retry] = useOnce<PipelineDetail>((signal) => client.get('/pipelines/{name}', { name }, { signal }));
  const onSelect = useCallback((id: string | null) => navigate(id === null ? { screen: 'editor', name } : { screen: 'editor', name, node: id }, { replace: true }), [name]);
  return (
    <Resource state={detail} loading={`Reading ${name}`} error={(problem) => <ErrorState problem={problem} onRetry={retry} />}>
      {(pipeline) => {
        if (pipeline.typed === null) {
          return (
            <Sheet>
              <EmptyState
                heading={`${name} cannot be opened on the canvas`}
                action={
                  <ButtonLink kind="primary" size="l" href={formatHash({ screen: 'editor' })}>
                    Start a new pipeline
                  </ButtonLink>
                }
              >
                Its text does not read as a pipeline document the canvas can hold{pipeline.error === null ? '.' : `: ${pipeline.error.detail}`}
              </EmptyState>
            </Sheet>
          );
        }
        if (workspace.status === 'loading') return <Loading label="Reading this build's capabilities" />;
        // A failed workspace read is the shell's to show, with Retry, above the screen.
        if (workspace.status === 'error') return null;
        return <Editing client={client} workspace={workspace.value} title={name} stored={name} initial={pipeline.typed} selected={node ?? null} onSelect={onSelect} />;
      }}
    </Resource>
  );
}

/**
 * `#editor`: one action, a new pipeline, and a link to Runs.
 * `#editor/<name>[/node/<id>]`: the stored pipeline on the canvas.
 */
export function EditorScreen({ client, name, node, workspace }: EditorScreenProps) {
  const [started, setStarted] = useState(false);
  if (name !== undefined) return <Stored key={name} client={client} name={name} node={node} workspace={workspace} />;
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
