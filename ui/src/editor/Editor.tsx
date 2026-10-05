import { useCallback, useEffect, useMemo, useReducer, useRef, useState, type KeyboardEvent } from 'react';
import { Button, Inspector } from '../../design/index.ts';
import type { ApiClient } from '../api/client.ts';
import type { Capabilities, ServiceStatus } from '../api/types.ts';
import { Canvas, edgeId, type CanvasPorts, type Position } from '../canvas/index.ts';
import { danglingInputs, freshId, toGraph, type WireDocument } from './document.ts';
import { EditorInspector, type NodeVerdict } from './EditorInspector.tsx';
import { InsertMenu, NodeEntries } from './menus.tsx';
import { Palette, paletteOf } from './Palette.tsx';
import { INPUT_KIND, portsOf, refusal, type PortGrammar } from './ports.ts';
import { asking, useLayoutSaving, useSaving } from './persist.ts';
import { runUpTo } from './prefix.ts';
import { isDirty, type FileInit, type SaveState } from './saving.ts';
import { ExportPanel, SavePrompt } from './SavePrompt.tsx';
import { canRedo, canUndo, editorReducer, initialEditor, type EditorAction, type EditorLayout } from './store.ts';
import { useValidation, type Verdict } from './validation.ts';
import { navigate } from '../routes.ts';
import './Editor.css';

// Where a node placed from the palette lands beside the selected one: a
// card's width and a rank gap to its right (canvas/layout.ts's constants).
const BESIDE = 224 + 64;

export type EditorProps = {
  client: ApiClient;
  /** The pipeline's name, or what stands for it. */
  title: string;
  /** The workspace pipeline the editor opened, by name; null or absent for a new document, which no run can take yet. */
  stored?: string | null;
  /** The wire-schema document the editor opens on. */
  initial: WireDocument;
  layout?: EditorLayout;
  /** `GET /workspace`'s capabilities: what the palette offers. */
  capabilities: Capabilities;
  /** The services Setup bound: the Remote names the palette offers. */
  services: readonly ServiceStatus[];
  /** Each node family's ports, when the API serves them; null, and no kind is refused during a drag. */
  grammar: PortGrammar | null;
  selected: string | null;
  onSelect: (id: string | null) => void;
  /** The file the editor writes, as it was read; absent, nothing is ever written. */
  file?: FileInit;
  /** The run this pipeline was forked from, named in the header. */
  forkedFrom?: string | null;
  /** The name the editor now writes under: a first creation, or a save as a new file. */
  onNamed?: (name: string) => void;
  /** "Discard my changes and reload": the file read again from disk. */
  onReload?: () => void;
};

// What the inspector's verdict slot says of a node the server names nothing about.
const QUIET: Record<'checking' | 'valid' | 'invalid' | 'failed', string> = {
  checking: 'Checking with the server…',
  valid: 'The server names no problem with this node.',
  invalid: 'The server stopped at a problem elsewhere; it has not judged this node past it.',
  failed: 'No verdict: the request to the server failed.',
};

// A field with an undo of its own: text being typed. A checkbox, a radio or a
// button has none, so Ctrl+Z there is the editor's.
const NO_TEXT = new Set(['checkbox', 'radio', 'button', 'submit', 'reset', 'range', 'color', 'file']);
const isTextField = (el: Element | null) => (el instanceof HTMLInputElement && !NO_TEXT.has(el.type)) || el instanceof HTMLTextAreaElement;

/** What the save line says: whether what is on the canvas is what is on disk, and if not, why. */
function saveWords(save: SaveState, dirty: boolean, verdict: Verdict, errors: number): string {
  const { phase } = save;
  switch (phase.kind) {
    case 'saving':
      return 'Saving…';
    case 'conflict':
      return 'Not saved: the file changed on disk';
    case 'handwritten':
      return 'Not saved: waiting for your choice';
    case 'taken':
      return 'Not saved: the name is taken';
    case 'failed':
      return `Not saved: ${phase.problem.message}`;
    case 'idle':
      if (verdict.status === 'invalid') return `Unsaved — ${errors} ${errors === 1 ? 'error' : 'errors'}`;
      if (verdict.status === 'failed') return 'Unsaved: the server could not be asked';
      if (dirty || (save.keep && save.file.name === null)) return 'Unsaved changes';
      return save.file.name === null ? 'Not written yet: the first change, or Keep this pipeline, writes it' : 'Saved';
  }
}

const shortRun = (id: string) => id.slice(0, 12);

/**
 * The editor: the canvas in write mode over one wire-schema document, the
 * palette beside it, the inspector for the selected node, undo and redo, the
 * server's verdict on the document as it stands, and — given the `file` it
 * writes — continuous saving (`persist.ts`, `saving.ts`) and the export.
 */
export function Editor({ client, title, stored = null, initial, layout, capabilities, services, grammar, selected, onSelect, file, forkedFrom = null, onNamed = () => {}, onReload = () => {} }: EditorProps) {
  const [state, apply] = useReducer(editorReducer, undefined, () => initialEditor(initial, layout));
  // A position a step changed is the person's and is written; where the
  // canvas placed a node itself on opening is kept, and written with the next.
  const [touched, setTouched] = useState(false);
  const dispatch = useCallback((action: EditorAction) => {
    if (action.type !== 'placed') setTouched(true);
    apply(action);
  }, []);
  const { doc } = state;
  const verdict = useValidation(client, doc);
  const [save, act] = useSaving(client, doc, verdict, file, onNamed);
  const written = save.file.etag === null ? null : save.file.name;
  // While a prompt is up, or a save as a new file is out, the positions wait for the file chosen.
  const holding = asking(save) || (save.phase.kind === 'saving' && save.phase.back !== null);
  const layoutFailed = useLayoutSaving(client, written, state.layout, touched, holding);
  const dirty = isDirty(save, doc);
  // Leaving the page while something is not on disk asks the browser to confirm.
  const unsaved = file !== undefined && (save.phase.kind !== 'idle' || dirty || (save.keep && save.file.name === null));
  useEffect(() => {
    if (!unsaved) return;
    const onLeave = (event: BeforeUnloadEvent) => event.preventDefault();
    window.addEventListener('beforeunload', onLeave);
    return () => window.removeEventListener('beforeunload', onLeave);
  }, [unsaved]);
  const [exporting, setExporting] = useState(false);
  const name = save.file.name ?? title;
  const fileName = save.file.name ?? (save.file.proposed === '' ? 'pipeline' : save.file.proposed);
  const root = useRef<HTMLDivElement>(null);
  const [inserting, setInserting] = useState<HTMLElement | null>(null);
  // A node to focus once it is on the canvas, or the inspector to enter once it opens.
  const [focus, setFocus] = useState<{ node: string } | { inspector: string } | null>(null);

  const graph = useMemo(() => toGraph(doc, grammar), [doc, grammar]);
  const ports = useMemo(() => {
    const out: Record<string, CanvasPorts> = {};
    for (const input of doc.pipeline.inputs) out[input] = { inputs: [], output: INPUT_KIND };
    for (const node of doc.pipeline.nodes) out[node.id] = portsOf(node, grammar);
    return out;
  }, [doc, grammar]);
  const sections = useMemo(() => paletteOf(capabilities, services), [capabilities, services]);
  const refuse = useCallback((from: string, to: string, port: number) => refusal(doc, grammar, from, to, port), [doc, grammar]);

  // The server's words on the node and the edge it located, and on every
  // consumer an input of which names nothing.
  const dangling = useMemo(() => danglingInputs(doc), [doc]);
  const located = verdict.status === 'invalid' ? verdict.problem.location : null;
  const issues = useMemo(() => {
    const out: Record<string, string> = { ...dangling };
    if (verdict.status === 'invalid' && located?.node != null) out[located.node] = verdict.problem.message;
    return out;
  }, [dangling, verdict, located]);
  const invalidEdges = useMemo(() => (located?.edge == null ? [] : [edgeId(located.edge.from, located.edge.to, located.edge.port)]), [located]);

  const verdictOf = (id: string): NodeVerdict => {
    if (verdict.status === 'invalid' && located?.node === id) return { message: verdict.problem.message, port: located.edge?.to === id ? located.edge.port : null };
    const node = doc.pipeline.nodes.find((n) => n.id === id);
    const known = new Set([...doc.pipeline.inputs, ...doc.pipeline.nodes.map((n) => n.id)]);
    const port = node?.inputs.findIndex((input) => !known.has(input)) ?? -1;
    return dangling[id] === undefined ? null : { message: dangling[id], port: port < 0 ? null : port };
  };

  const place = (component: string, impl: string, position?: Position) => {
    const at = position ?? (selected === null ? undefined : state.layout[selected]);
    const beside = position === undefined && at !== undefined ? { x: at.x + BESIDE, y: at.y } : at;
    // The id the store gives it, from the same document.
    const id = freshId(doc, impl);
    dispatch({ type: 'add', component, impl, ...(beside === undefined ? {} : { position: beside }) });
    onSelect(id);
    setFocus({ node: id });
  };

  // Whether an id names something in the document as it stands.
  const exists = (id: string) => doc.pipeline.inputs.includes(id) || doc.pipeline.nodes.some((n) => n.id === id);

  // A selection undo or redo took away — a node placed, renamed or
  // duplicated, then undone — is cleared, never left on an id nothing has.
  useEffect(() => {
    if (selected !== null && !exists(selected)) onSelect(null);
  });

  // A focus request waits for its target to be drawn, and expires once its
  // target is gone, so a later redo bringing it back takes no focus.
  useEffect(() => {
    if (focus === null) return;
    if (('node' in focus && !exists(focus.node)) || ('inspector' in focus && focus.inspector !== selected)) {
      setFocus(null);
      return;
    }
    if ('node' in focus) {
      // The canvas's node element: the one place the editor reaches into the canvas's markup, to give focus to what it just placed.
      const el = root.current?.querySelector<HTMLElement>(`.react-flow__node[data-id="${CSS.escape(focus.node)}"]`);
      if (el != null) {
        el.focus();
        setFocus(null);
      }
    } else {
      const field = root.current?.querySelector<HTMLElement>(`.rg-canvas__inspector input`);
      if (field != null) {
        field.focus();
        setFocus(null);
      }
    }
  });

  // Undo and redo from the keyboard: inside the editor, and with focus on
  // the page itself, where it lands when what had it went away.
  const history = (event: { key: string; ctrlKey: boolean; metaKey: boolean; shiftKey: boolean; target: EventTarget | null; preventDefault: () => void }) => {
    if (!(event.ctrlKey || event.metaKey) || isTextField(event.target as Element)) return;
    const key = event.key.toLowerCase();
    if (key === 'z' && !event.shiftKey) {
      event.preventDefault();
      dispatch({ type: 'undo' });
    } else if ((key === 'z' && event.shiftKey) || key === 'y') {
      event.preventDefault();
      dispatch({ type: 'redo' });
    }
  };
  const onKeyDown = (event: KeyboardEvent<HTMLDivElement>) => history(event);
  useEffect(() => {
    const onPage = (event: globalThis.KeyboardEvent) => {
      if (event.target === document.body) history(event);
    };
    document.addEventListener('keydown', onPage);
    return () => document.removeEventListener('keydown', onPage);
  }, []);

  // "Run up to this node" launches the document on disk cut at the node, so it is offered only while the canvas holds
  // exactly what is on disk: a run takes the file, never the canvas. Given the file it writes, that is the file it
  // last read or wrote, with nothing pending; with none, the stored pipeline it opened, still unedited.
  const onDisk = file === undefined ? stored : save.file.etag === null ? null : save.file.name;
  const pristine = useMemo(() => JSON.stringify(doc) === JSON.stringify(initial), [doc, initial]);
  const unchanged = file === undefined ? pristine : !isDirty(save, doc) && save.phase.kind === 'idle';
  const runOf = (id: string) => runUpTo(doc, id, { name: onDisk, unchanged });
  const runUpToNode = (id: string) => {
    if (onDisk !== null) navigate({ screen: 'runs', launch: { pipeline: onDisk, upTo: id } });
  };

  const nodeOf = (id: string) => doc.pipeline.nodes.find((n) => n.id === id);
  const inspector = (id: string) => {
    const node = nodeOf(id);
    // Neither a node nor an input: a selection about to be cleared draws nothing.
    if (node === undefined && !doc.pipeline.inputs.includes(id)) return null;
    if (node === undefined) {
      return (
        <Inspector family="query" title={id} impl="pipeline input">
          <p className="rg-editor-inspector__none">A declared input: the query the pipeline receives. It consumes nothing.</p>
        </Inspector>
      );
    }
    return (
      <EditorInspector
        key={id}
        doc={doc}
        node={node}
        ports={portsOf(node, grammar)}
        verdict={verdictOf(id)}
        quiet={QUIET[verdict.status]}
        dispatch={dispatch}
        onRenamed={onSelect}
        run={runOf(id)}
        onRunUpTo={() => runUpToNode(id)}
      />
    );
  };
  const menu = (id: string, close: () => void) =>
    nodeOf(id) === undefined ? (
      doc.pipeline.inputs.includes(id) ? (
        <NodeEntries doc={doc} grammar={grammar} node={id} input refuse={refuse} close={close} onOpen={() => {}} onDuplicate={() => {}} onDelete={() => {}} onConnect={(to, port) => dispatch({ type: 'connect', from: id, to, port })} />
      ) : null
    ) : (
      <NodeEntries
        doc={doc}
        grammar={grammar}
        node={id}
        refuse={refuse}
        run={runOf(id)}
        onRunUpTo={() => runUpToNode(id)}
        close={close}
        onOpen={() => {
          onSelect(id);
          setFocus({ inspector: id });
        }}
        onDuplicate={() => {
          const copy = freshId(doc, nodeOf(id)!.impl);
          dispatch({ type: 'duplicate', node: id });
          onSelect(copy);
        }}
        onConnect={(to, port) => dispatch({ type: 'connect', from: id, to, port })}
        onDelete={() => {
          // Focus goes to a neighbour — what fed the node, else the first
          // input — rather than to the page with the node's element.
          const neighbour = nodeOf(id)!.inputs.find((i) => i !== id && (nodeOf(i) !== undefined || doc.pipeline.inputs.includes(i))) ?? doc.pipeline.inputs[0];
          dispatch({ type: 'remove', node: id });
          if (selected === id) onSelect(null);
          if (neighbour !== undefined) setFocus({ node: neighbour });
        }}
      />
    );

  const header = (() => {
    switch (verdict.status) {
      case 'checking':
        return <span>Checking with the server…</span>;
      case 'valid':
        return (
          <span>
            Canonical hash <code title="The identity a run of this document would carry">{verdict.hash}</code>
          </span>
        );
      case 'invalid':
        return <span data-invalid="true">Not valid: {verdict.problem.message}</span>;
      case 'failed':
        return <span data-invalid="true">Could not validate: {verdict.problem.message}</span>;
    }
  })();
  const rendering = verdict.status === 'valid' ? verdict.rendering : null;
  const errors = Math.max(1, Object.keys(issues).length);
  // Said once, politely: what is wrong, and nothing while the document is valid or being checked.
  const announced = verdict.status === 'invalid' ? `Not valid: ${verdict.problem.message}` : verdict.status === 'failed' ? `Could not validate: ${verdict.problem.message}` : '';

  return (
    <div ref={root} className="rg-editor" onKeyDown={onKeyDown}>
      <header className="rg-editor__bar">
        <h2 className="rg-editor__title">{name}</h2>
        <div className="rg-editor__history" role="group" aria-label="History">
          <Button kind="quiet" size="s" icon="undo" onClick={() => dispatch({ type: 'undo' })} {...(canUndo(state) ? {} : { disabled: true, disabledReason: 'Nothing to undo.' })}>
            Undo
          </Button>
          <Button kind="quiet" size="s" icon="redo" onClick={() => dispatch({ type: 'redo' })} {...(canRedo(state) ? {} : { disabled: true, disabledReason: 'Nothing to redo.' })}>
            Redo
          </Button>
        </div>
        <Button kind="quiet" size="s" aria-expanded={exporting} onClick={() => setExporting(!exporting)} {...(rendering === null ? { disabled: true, disabledReason: 'Only a document the server calls valid is exported.' } : {})}>
          Export
        </Button>
        <p className="rg-editor__verdict">{header}</p>
        <p className="rg-visually-hidden" role="status">
          {announced}
        </p>
        {file === undefined ? null : (
          <div className="rg-editor__file">
            <p className="rg-editor__save" data-testid="save-state" role="status" aria-live="polite">
              {saveWords(save, dirty, verdict, errors)}
            </p>
            {save.file.name === null && !save.keep ? (
              <Button size="s" onClick={() => act({ type: 'keep' })}>
                Keep this pipeline
              </Button>
            ) : null}
            {forkedFrom === null ? null : (
              <p className="rg-editor__fork">
                Forked from run <code title={forkedFrom}>{shortRun(forkedFrom)}</code>
              </p>
            )}
            {layoutFailed === null ? null : <p className="rg-editor__layout-failed">The positions could not be saved: {layoutFailed.message}</p>}
          </div>
        )}
      </header>
      <SavePrompt
        phase={save.phase}
        name={save.file.name ?? save.file.proposed}
        valid={verdict.status === 'valid'}
        onReload={onReload}
        onRewrite={() => act({ type: 'rewrite' })}
        onSaveAs={(to) => {
          // A new file is written only from a document the server called valid, as every write is.
          if (verdict.status === 'valid') act({ type: 'saveAs', doc, name: to });
        }}
      />
      {exporting && rendering !== null ? <ExportPanel name={fileName} rendering={rendering} /> : null}
      <div className="rg-editor__body">
        <Palette entries={sections} onPlace={(component, impl) => place(component, impl)} />
        <div className="rg-editor__stage">
          <Canvas
            graph={graph}
            label={`Pipeline ${name}`}
            mode="write"
            layout={state.layout}
            selected={selected}
            onSelect={onSelect}
            onAutoPlaced={(positions) => dispatch({ type: 'placed', positions })}
            inspector={inspector}
            menu={menu}
            ports={ports}
            refuse={refuse}
            onConnect={(from, to, port) => dispatch({ type: 'connect', from, to, port })}
            onMove={(node, position) => dispatch({ type: 'move', node, position })}
            onInsert={() => setInserting(document.activeElement as HTMLElement | null)}
            onDropItem={(item, position) => {
              const { component, impl } = JSON.parse(item) as { component: string; impl: string };
              const entry = sections.flatMap((s) => s.entries).find((e) => e.component === component && e.impl === impl);
              if (entry !== undefined && entry.refused === null) place(component, impl, position);
            }}
            issues={issues}
            invalidEdges={invalidEdges}
          />
          {inserting === null ? null : (
            <InsertMenu
              sections={sections}
              onPlace={(component, impl) => {
                setInserting(null);
                place(component, impl);
              }}
              onClose={() => {
                inserting.focus();
                setInserting(null);
              }}
            />
          )}
        </div>
      </div>
    </div>
  );
}
