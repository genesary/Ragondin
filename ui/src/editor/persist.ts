// The editor's writes, driven by the state machine in saving.ts: the document
// after the server called it valid (`PUT /pipelines/{name}` with the typed
// document, which the server renders and stores), and the layout beside it on
// every position change (`PUT /pipelines/{name}/layout`), which is never
// invalid and never hashed, so it needs no gate (ADR-016 § 4).
// ARCHITECTURE.md § The editor.
import { useEffect, useReducer, useRef, useState } from 'react';
import type { ApiClient, ApiProblem } from '../api/client.ts';
import type { WireDocument } from './document.ts';
import { initialSave, saveReducer, type FileInit, type SaveEvent, type SaveState } from './saving.ts';
import { chooseRewrite, rewriteChosen } from './session.ts';
import type { EditorLayout } from './store.ts';
import type { Verdict } from './validation.ts';

/** How long positions must rest before the layout is written: a drag or a run of arrow keys is one write. */
export const LAYOUT_DEBOUNCE_MS = 250;

const NOTHING: FileInit = { name: null, etag: null, canonical: true, proposed: '' };

/**
 * The saving state of `doc`, given the server's verdict on it. With no
 * `file`, nothing is ever written. `onNamed` hears the name the editor now
 * writes under, whenever it changes: a first creation, or a save as a new file.
 */
export function useSaving(client: ApiClient, doc: WireDocument, verdict: Verdict, file: FileInit | undefined, onNamed: (name: string) => void): [SaveState, (event: SaveEvent) => void] {
  const enabled = file !== undefined;
  const [state, dispatch] = useReducer(saveReducer, undefined, () => initialSave(doc, file ?? NOTHING, file?.name != null && rewriteChosen(file.name)));
  const { phase } = state;

  // Every valid document is offered; the state machine decides whether it is written.
  useEffect(() => {
    if (enabled && verdict.status === 'valid') dispatch({ type: 'valid', doc });
  }, [enabled, verdict, doc, phase, state.file.handwritten, state.keep]);

  // A write, once sent, is never cancelled: its answer is what says what is on disk.
  const etag = useRef(state.file.etag);
  etag.current = state.file.etag;
  useEffect(() => {
    if (phase.kind !== 'saving') return;
    const headers = phase.create ? { 'If-None-Match': '*' } : { 'If-Match': `"${etag.current ?? ''}"` };
    void client.put('/pipelines/{name}', { typed: phase.doc }, { name: phase.name }, { headers }).then((result) => {
      dispatch(result.ok ? { type: 'written', doc: phase.doc, name: result.value.name, etag: result.value.etag } : { type: 'refused', problem: result.problem });
    });
  }, [client, phase]);

  const named = useRef(file?.name ?? null);
  useEffect(() => {
    if (state.file.name !== null && state.file.name !== named.current) {
      named.current = state.file.name;
      onNamed(state.file.name);
    }
  });

  const act = (event: SaveEvent) => {
    if (event.type === 'rewrite' && state.file.name !== null) chooseRewrite(state.file.name);
    dispatch(event);
  };
  return [state, act];
}

/**
 * Writes `layout` beside the file `name` once a step — not the canvas's own
 * automatic placement on opening — has changed it, debounced. Answers why the
 * last write failed, if it did.
 */
export function useLayoutSaving(client: ApiClient, name: string | null, layout: EditorLayout, touched: boolean): ApiProblem | null {
  const saved = useRef<string | null>(null);
  const [failed, setFailed] = useState<ApiProblem | null>(null);
  useEffect(() => {
    if (name === null || !touched) return;
    const text = JSON.stringify(layout);
    if (text === saved.current) return;
    const timer = setTimeout(() => {
      void client.put('/pipelines/{name}/layout', { version: 1, nodes: { ...layout } }, { name }).then((result) => {
        if (result.ok) {
          saved.current = text;
          setFailed(null);
        } else setFailed(result.problem);
      });
    }, LAYOUT_DEBOUNCE_MS);
    return () => clearTimeout(timer);
  }, [client, name, layout, touched]);
  return failed;
}
