// The Editor screen: the canvas in write mode over a pipeline document
// (src/editor/Editor.tsx) — a new one, the first-launch example, an import, or
// a stored one opened from its typed document — written back as it changes.
// ARCHITECTURE.md § The editor.
import { lazy, Suspense, useCallback, useEffect, useMemo, useRef, useState, type ReactNode } from 'react';
import { Button, ButtonLink, EmptyState, Sheet } from '../../design/index.ts';
import type { ApiClient, ApiResult } from '../api/client.ts';
import type { PipelineDetail, PipelineLayout, PipelineListing, ServiceListing, Workspace } from '../api/types.ts';
import { formatHash, navigate } from '../routes.ts';
import { ErrorState, Loading, Resource, type RequestState } from '../shell/states.tsx';
import { emptyDocument, type WireDocument } from './document.ts';
import { exampleDocument, freshName } from './example.ts';
import { ImportPanel } from './Import.tsx';
import { grammarOf } from './ports.ts';
import type { FileInit } from './saving.ts';
import { PipelinePicker, pickerOrder } from './PipelinePicker.tsx';
import { recentPipelines, rememberPipeline } from './recent.ts';
import { forkedFrom } from './session.ts';
import type { EditorLayout } from './store.ts';

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

/** What a session's editor opens on, and the file it writes. */
type Opening = { title: string; doc: WireDocument; layout?: EditorLayout; file: FileInit; forkedFrom: string | null };

type Callbacks = { onNamed: (name: string) => void; onReload: () => void };

/** The editor over `opening`, once the services are read. */
function Editing({ client, workspace, opening, selected, onSelect, onNamed, onReload }: { client: ApiClient; workspace: Workspace; opening: Opening; selected: string | null; onSelect: (id: string | null) => void } & Callbacks) {
  const [services, retry] = useOnce<ServiceListing>((signal) => client.get('/services', { signal }));
  // The workspace's pipelines, for the header's picker: a listing that fails offers none, and the editor works on.
  const [listing] = useOnce<PipelineListing>((signal) => client.get('/pipelines', { signal }));
  const names = listing.status === 'loaded' ? listing.value.pipelines.map((p) => p.name) : [];
  const grammar = useMemo(() => grammarOf(workspace.capabilities), [workspace.capabilities]);
  return (
    <Resource state={services} loading="Reading the services Setup bound" error={(problem) => <ErrorState problem={problem} onRetry={retry} />}>
      {(listing) => (
        <Suspense fallback={<Loading label="Opening the editor" />}>
          <Editor
            client={client}
            title={opening.title}
            initial={opening.doc}
            {...(opening.layout === undefined ? {} : { layout: opening.layout })}
            capabilities={workspace.capabilities}
            services={listing.services}
            grammar={grammar}
            selected={selected}
            onSelect={onSelect}
            file={opening.file}
            forkedFrom={opening.forkedFrom}
            onNamed={onNamed}
            onReload={onReload}
            picker={(current) => (names.length === 0 ? null : <PipelinePicker id="rg-editor-open" names={names} current={current} />)}
          />
        </Suspense>
      )}
    </Resource>
  );
}

/** The capabilities, or what to show while they are not there. */
function withWorkspace(workspace: RequestState<Workspace>, render: (workspace: Workspace) => ReactNode) {
  if (workspace.status === 'loading') return <Loading label="Reading this build's capabilities" />;
  // A failed workspace read is the shell's to show, with Retry, above the screen.
  if (workspace.status === 'error') return null;
  return render(workspace.value);
}

/**
 * `#editor`: on a workspace with no pipeline file, the first-launch example,
 * written at its first edit or at "Keep this pipeline"; otherwise one action,
 * a new pipeline, beside an import and a link to Runs. A document never
 * written is first written under a name free in the listing; its selection is
 * the screen's, since no address names it.
 */
function Unnamed({ client, workspace, ...callbacks }: { client: ApiClient; workspace: RequestState<Workspace> } & Callbacks) {
  const [chosen, setChosen] = useState<'new' | 'import' | null>(null);
  const [selected, setSelected] = useState<string | null>(null);
  // The shell's count says whether the workspace may be empty; the listing
  // decides, since the count is as old as the shell's last read, and gives
  // the names a new document's first write must avoid. A workspace the count
  // says holds pipelines is not listed until a new document needs a name.
  const maybeEmpty = workspace.status === 'loaded' && workspace.value.counts.pipelines === 0;
  // Listed always: the empty state offers the workspace's pipelines to open, the recent ones first.
  const [listing] = useOnce<PipelineListing>(workspace.status === 'loaded' ? (signal) => client.get('/pipelines', { signal }) : null);
  const listed = maybeEmpty || chosen === 'new';
  if (listed && listing.status === 'loading') return <Loading label="Reading the workspace's pipelines" />;
  // A listing that failed proposes names blind; the server still refuses one taken.
  const names = listing.status === 'loaded' ? listing.value.pipelines.map((p) => p.name) : [];
  const empty = maybeEmpty && listing.status === 'loaded' && names.length === 0;

  if (chosen === 'import') {
    return (
      <Sheet>
        <ImportPanel client={client} onImported={(name) => navigate({ screen: 'editor', name })} onCancel={() => setChosen(null)} />
      </Sheet>
    );
  }
  if (chosen === 'new' || empty) {
    return withWorkspace(workspace, (ws) => {
      const example = chosen === null ? exampleDocument(ws.capabilities) : null;
      const opening: Opening =
        example === null
          ? { title: 'New pipeline', doc: emptyDocument(), file: { name: null, etag: null, canonical: true, proposed: freshName('pipeline', names) }, forkedFrom: null }
          : { title: 'Example pipeline', doc: example, file: { name: null, etag: null, canonical: true, proposed: freshName('example', names) }, forkedFrom: null };
      return <Session opening={opening} client={client} workspace={ws} selected={selected} onSelect={setSelected} {...callbacks} />;
    });
  }
  return (
    <Sheet>
      <EmptyState
        heading="No pipeline open"
        action={
          <>
            <Button kind="primary" size="l" onClick={() => setChosen('new')}>
              Start a new pipeline
            </Button>
            <Button size="l" onClick={() => setChosen('import')}>
              Import a pipeline
            </Button>
            <ButtonLink kind="quiet" size="l" href={formatHash({ screen: 'runs' })}>
              Open Runs
            </ButtonLink>
          </>
        }
      >
        Start a new pipeline on the canvas, from what this build can run, or import a pipeline document.
      </EmptyState>
      {listing.status === 'loaded' && names.length > 0 ? <OpenOne names={names} /> : null}
    </Sheet>
  );
}

/** The empty state's way to an existing pipeline: the recent ones as links, every one in a picker. */
function OpenOne({ names }: { names: readonly string[] }) {
  const recent = pickerOrder(names).filter((n) => recentPipelines().includes(n));
  return (
    <section className="rg-editor__open" aria-label="Open a pipeline">
      {recent.length === 0 ? null : (
        <>
          <h3 id="rg-editor-recent" className="rg-editor__open-head">
            Recent pipelines
          </h3>
          <ul className="rg-editor__recent" aria-labelledby="rg-editor-recent">
            {recent.map((n) => (
              <li key={n}>
                <a href={formatHash({ screen: 'editor', name: n })}>{n}</a>
              </li>
            ))}
          </ul>
        </>
      )}
      <PipelinePicker id="rg-editor-open-empty" names={names} current={null} />
    </section>
  );
}

/** The editor, its opening fixed when it mounts: what it writes afterwards is its own. */
function Session({ opening, ...rest }: { opening: Opening; client: ApiClient; workspace: Workspace; selected: string | null; onSelect: (id: string | null) => void } & Callbacks) {
  const [fixed] = useState(opening);
  return <Editing opening={fixed} {...rest} />;
}

const layoutOf = (read: RequestState<PipelineLayout>): EditorLayout | undefined => (read.status === 'loaded' && read.value.layout !== null ? read.value.layout.nodes : undefined);

/**
 * A stored pipeline, opened from the typed document `GET /pipelines/{name}`
 * serves — whether or not it validates (ADR-C40 § 4) — at the positions its
 * layout holds, its selected node the address's: restored from
 * `#editor/<name>/node/<id>`, and written there. A document whose text does
 * not read into the wire schema, or holds a value the typed document cannot
 * carry, has none, and is said to be text only.
 */
function Stored({ client, name, current, node, workspace, ...callbacks }: { client: ApiClient; name: string; current: string | undefined; node: string | undefined; workspace: RequestState<Workspace> } & Callbacks) {
  const [detail, retry] = useOnce<PipelineDetail>((signal) => client.get('/pipelines/{name}', { name }, { signal }));
  // Positions are presentation: a layout that cannot be read opens the
  // pipeline laid out by the canvas, never keeps it closed.
  const [layout] = useOnce<PipelineLayout>((signal) => client.get('/pipelines/{name}/layout', { name }, { signal }));
  // The address names the file the editor writes now: a save as a new file changes it.
  const at = current ?? name;
  const onSelect = useCallback((id: string | null) => navigate(id === null ? { screen: 'editor', name: at } : { screen: 'editor', name: at, node: id }, { replace: true }), [at]);
  if (layout.status === 'loading') return <Loading label={`Reading ${name}`} />;
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
        const stored = layoutOf(layout);
        rememberPipeline(pipeline.name);
        const opening: Opening = {
          title: name,
          doc: pipeline.typed,
          ...(stored === undefined ? {} : { layout: stored }),
          file: { name: pipeline.name, etag: pipeline.etag, canonical: pipeline.canonical, proposed: pipeline.name },
          forkedFrom: forkedFrom(pipeline.name),
        };
        return withWorkspace(workspace, (ws) => <Session opening={opening} client={client} workspace={ws} selected={node ?? null} onSelect={onSelect} {...callbacks} />);
      }}
    </Resource>
  );
}

/** One mounted editor: what it opened on is read once, when it mounts. */
function Opened({ client, name, node, workspace, ...callbacks }: EditorScreenProps & Callbacks) {
  const [opened] = useState(name);
  if (opened === undefined) return <Unnamed client={client} workspace={workspace} {...callbacks} />;
  return <Stored client={client} name={opened} current={name} node={node} workspace={workspace} {...callbacks} />;
}

/**
 * `#editor` and `#editor/<name>[/node/<id>]`. The address moving to the file
 * the mounted editor has just written — its first creation, or a save as a
 * new file — keeps that editor and its history; moving anywhere else, or
 * "Discard my changes and reload", opens afresh from disk.
 */
export function EditorScreen({ client, name, node, workspace }: EditorScreenProps) {
  const [session, setSession] = useState<{ key: number; route: string | undefined; owned: string | null }>({ key: 0, route: name, owned: null });
  if (session.route !== name) {
    const kept = name !== undefined && name === session.owned;
    setSession({ key: kept ? session.key : session.key + 1, route: name, owned: kept ? session.owned : null });
  }
  const onNamed = useCallback((written: string) => {
    setSession((s) => ({ ...s, owned: written }));
    navigate({ screen: 'editor', name: written }, { replace: true });
  }, []);
  const onReload = useCallback(() => setSession((s) => ({ ...s, key: s.key + 1 })), []);
  return <Opened key={session.key} client={client} name={name} node={node} workspace={workspace} onNamed={onNamed} onReload={onReload} />;
}
