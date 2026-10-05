// The editor's writes, driven by the state machine in saving.ts: the document
// after the server called it valid (`PUT /pipelines/{name}` with the typed
// document, which the server renders and stores), and the layout beside it on
// every position change (`PUT /pipelines/{name}/layout`), which is never
// invalid and never hashed, so it needs no gate (ADR-016 § 4). When the editor
// closes, what is pending is flushed under the same protections: the last
// document is validated and written over the etag held, the last positions
// written beside the file; nothing held behind a prompt is.
// ARCHITECTURE.md § The editor.
import { useEffect, useReducer, useRef, useState } from 'react';
import type { ApiClient, ApiProblem } from '../api/client.ts';
import type { WireDocument } from './document.ts';
import { initialSave, saveReducer, type FileInit, type SaveEvent, type SaveState } from './saving.ts';
import { renameRecent } from './recent.ts';
import { chooseRewrite, renameSession, rewriteChosen } from './session.ts';
import type { EditorLayout } from './store.ts';
import type { Verdict } from './validation.ts';

/** How long positions must rest before the layout is written: a drag or a run of arrow keys is one write. */
export const LAYOUT_DEBOUNCE_MS = 250;

const NOTHING: FileInit = { name: null, etag: null, canonical: true, proposed: '' };

/** What is on disk as far as the editor knows, updated the moment a write answers — before the render that follows, and after the editor is gone. */
type Disk = { name: string | null; etag: string | null; saved: string; blocked: boolean };

/** The phases that wait on the person: nothing is written while one is up. */
export const asking = (state: SaveState) => state.phase.kind === 'conflict' || state.phase.kind === 'handwritten' || state.phase.kind === 'naming';

/** A rename out, and what the last one done did not take with it. */
export type RenameState = { renaming: boolean; fault: string | null };

/**
 * Writes what the editor held when it closed: `doc`, validated first unless
 * `verdict` already called it valid, over the etag of the last write — once
 * the write in flight, if any, has answered. Nothing is written while the
 * stale-etag or the hand-written prompt is up, for a hand-written file not yet asked about, after a stale
 * etag, or for a document the server refuses. A document never written is
 * created under the name its question offers, if that name is free.
 */
async function flush(client: ApiClient, doc: WireDocument, verdict: Verdict, state: SaveState, disk: { current: Disk }, inflight: Promise<void> | null) {
  // A document never written whose name is being asked for is kept under the name offered rather than lost: it is
  // created, never written over another. The two other questions hold everything back.
  if ((asking(state) && state.phase.kind !== 'naming') || state.file.handwritten) return;
  if (inflight !== null) await inflight;
  const at = disk.current;
  if (at.blocked) return;
  const text = JSON.stringify(doc);
  if (text === at.saved && !(at.name === null && state.keep)) return;
  if (state.phase.kind === 'failed' && JSON.stringify(state.phase.doc) === text) return;
  if (verdict.status !== 'valid') {
    const checked = await client.post('/pipelines/validate', { typed: doc });
    if (!checked.ok) return;
  }
  const name = at.name ?? state.file.proposed;
  const headers = at.etag === null ? { 'If-None-Match': '*' } : { 'If-Match': `"${at.etag}"` };
  await client.put('/pipelines/{name}', { typed: doc }, { name }, { headers });
}

/**
 * The saving state of `doc`, given the server's verdict on it. With no
 * `file`, nothing is ever written. `onNamed` hears the name the editor now
 * writes under, whenever it changes: a first creation, or a save as a new file.
 * The fourth value says whether a rename is out — nothing is written until
 * it answers, so no write lands under the name it moves away from — and what
 * the last rename, done, could not take with it (`fault`), in the server's words.
 */
export function useSaving(
  client: ApiClient,
  doc: WireDocument,
  verdict: Verdict,
  file: FileInit | undefined,
  onNamed: (name: string) => void,
): [SaveState, (event: SaveEvent) => void, (to: string) => Promise<string | null>, RenameState] {
  const enabled = file !== undefined;
  const [state, dispatch] = useReducer(saveReducer, undefined, () => initialSave(doc, file ?? NOTHING, file?.name != null && rewriteChosen(file.name)));
  const { phase } = state;
  const disk = useRef<Disk>({ name: state.file.name, etag: state.file.etag, saved: state.saved, blocked: false });
  const inflight = useRef<Promise<void> | null>(null);
  const [renaming, setRenaming] = useState(false);
  const [fault, setFault] = useState<string | null>(null);
  const latest = useRef({ client, doc, verdict, state });
  latest.current = { client, doc, verdict, state };

  // Every valid document is offered, once no rename is out; the state machine decides whether it is written.
  useEffect(() => {
    if (enabled && !renaming && verdict.status === 'valid') dispatch({ type: 'valid', doc });
  }, [enabled, renaming, verdict, doc, phase, state.file.handwritten, state.keep]);

  // A write, once sent, is never cancelled: its answer is what says what is on disk.
  useEffect(() => {
    if (phase.kind !== 'saving') return;
    const headers = phase.create ? { 'If-None-Match': '*' } : { 'If-Match': `"${disk.current.etag ?? ''}"` };
    inflight.current = client.put('/pipelines/{name}', { typed: phase.doc }, { name: phase.name }, { headers }).then((result) => {
      if (result.ok) disk.current = { name: result.value.name, etag: result.value.etag, saved: JSON.stringify(phase.doc), blocked: false };
      else if (result.problem.code === 'precondition_failed' && phase.back === null) disk.current = { ...disk.current, blocked: true };
      dispatch(result.ok ? { type: 'written', doc: phase.doc, name: result.value.name, etag: result.value.etag } : { type: 'refused', problem: result.problem });
    });
  }, [client, phase]);

  // Leaving the editor — another address, another pipeline, a reload of this one — flushes it.
  useEffect(() => {
    if (!enabled) return;
    return () => {
      const { client, doc, verdict, state } = latest.current;
      void flush(client, doc, verdict, state, disk, state.phase.kind === 'saving' ? inflight.current : null);
    };
  }, [enabled]);

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

  /**
   * The title edited: a document never written takes the name for its first write; a file is renamed on disk
   * (`POST /pipelines/{name}/rename`) over the etag held — its layout and pairings move with it, its bytes and so
   * its etag unchanged — and the editor writes on under the new name. Answers why it was refused, or null.
   */
  const rename = async (to: string): Promise<string | null> => {
    const from = disk.current.name;
    if (from === null) {
      dispatch({ type: 'propose', name: to });
      return null;
    }
    if (to === from) return null;
    if (latest.current.state.phase.kind === 'saving') return 'A save is under way: rename once it has answered.';
    if (asking(latest.current.state)) return 'Answer the question above first.';
    setRenaming(true);
    const result = await client.post('/pipelines/{name}/rename', { to }, { name: from }, { headers: { 'If-Match': `"${disk.current.etag ?? ''}"` } });
    // Lifted in the same update as the new name lands, so nothing held is written under the old one.
    setRenaming(false);
    if (!result.ok) {
      if (result.problem.code === 'precondition_failed') return `${from}.yaml changed on disk since the editor read it: reload it, then rename it.`;
      return result.problem.code === 'pipeline_exists' ? `A pipeline named \`${to}\` already exists: choose another name.` : result.problem.message;
    }
    disk.current = { ...disk.current, name: result.value.name };
    setFault(result.value.fault);
    renameSession(from, result.value.name);
    renameRecent(from, result.value.name);
    dispatch({ type: 'renamed', name: result.value.name });
    return null;
  };
  return [state, act, rename, { renaming, fault }];
}

/**
 * Writes `layout` beside the file `name` once a step — not the canvas's own
 * automatic placement on opening — has changed it, debounced, and at once when
 * the editor closes. Nothing is written while `paused` (a prompt is up); once
 * it lifts, the positions go beside whichever file was chosen. Answers why the
 * last write failed, if it did.
 */
export function useLayoutSaving(client: ApiClient, name: string | null, layout: EditorLayout, touched: boolean, paused: boolean): ApiProblem | null {
  // The file and the positions last written beside it: a save as a new file writes them again.
  const saved = useRef<string | null>(null);
  const pending = useRef<{ name: string; layout: EditorLayout; key: string } | null>(null);
  const [failed, setFailed] = useState<ApiProblem | null>(null);
  const live = useRef(true);

  const write = (next: { name: string; layout: EditorLayout; key: string }) =>
    client.put('/pipelines/{name}/layout', { version: 1, nodes: { ...next.layout } }, { name: next.name }).then((result) => {
      if (result.ok) saved.current = next.key;
      if (live.current) setFailed(result.ok ? null : result.problem);
    });

  useEffect(() => {
    pending.current = null;
    if (name === null || !touched || paused) return;
    const key = `${name}\n${JSON.stringify(layout)}`;
    if (key === saved.current) return;
    const next = { name, layout, key };
    pending.current = next;
    const timer = setTimeout(() => {
      pending.current = null;
      void write(next);
    }, LAYOUT_DEBOUNCE_MS);
    return () => clearTimeout(timer);
  });

  useEffect(() => {
    live.current = true;
    return () => {
      live.current = false;
      if (pending.current !== null) void write(pending.current);
    };
  }, []);
  return failed;
}
