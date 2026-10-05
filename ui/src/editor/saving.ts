// Continuous saving: the state machine that decides when the editor writes its
// document to `pipelines/<name>.yaml`, and what it waits for. There is no Save
// button: a document the server called valid is written once it differs from
// what was last read or written, over the etag of that read or write; an
// invalid one never is (ADR-016 § 3). Two things stop the writing until the
// person chooses: a stale etag — the file changed on disk — and a file whose
// text is not the server's own rendering (ADR-016 § 5). Nothing here makes a
// request: `Editor.tsx` turns a `saving` phase into a `PUT` and hands the
// answer back. ARCHITECTURE.md § The editor.
import type { ApiProblem } from '../api/client.ts';
import type { WireDocument } from './document.ts';

/** What the editor knows of the file it writes, when it opens. */
export type FileInit = {
  /** The pipeline's name; null for a document never written (a new one, the first-launch example). */
  name: string | null;
  /** The etag the document was read at; null when there is no file yet. */
  etag: string | null;
  /** Whether the file's text is the server's own rendering (`GET /pipelines/{name}`'s `canonical`). */
  canonical: boolean;
  /** The name an unnamed document is first written under. */
  proposed: string;
};

/** Where a write asking for a new name goes back to when it is refused. */
export type Choice = 'conflict' | 'handwritten' | 'taken';

export type Phase =
  /** Nothing to write, or waiting for the next valid document. */
  | { kind: 'idle' }
  /** A `PUT` of `doc` to `name` is out: over the etag held, or creating it when `create`. `back` is set for a save under a new name. */
  | { kind: 'saving'; doc: WireDocument; name: string; create: boolean; back: Choice | null }
  /** The file changed on disk since it was read: reload it or keep the canvas as a new file. */
  | { kind: 'conflict'; error?: string }
  /** The file was written by hand: rewrite it, or save the canvas as a new file. */
  | { kind: 'handwritten'; error?: string }
  /** The name proposed for a new document was taken meanwhile: give another. */
  | { kind: 'taken'; error?: string }
  /** The server refused the write of `doc` otherwise; the next different document is tried again. */
  | { kind: 'failed'; doc: WireDocument; problem: ApiProblem };

export type SaveState = {
  file: { name: string | null; etag: string | null; handwritten: boolean; proposed: string };
  /** The document last read or written, as JSON: what is on disk as far as the editor knows. */
  saved: string;
  /** "Keep this pipeline": write an unnamed document though it is unedited. */
  keep: boolean;
  phase: Phase;
};

export type SaveEvent =
  /** The server called `doc` valid: the document as it stands now. */
  | { type: 'valid'; doc: WireDocument }
  /** The `PUT` in flight stored `doc` under `name`, now at `etag`. */
  | { type: 'written'; doc: WireDocument; name: string; etag: string }
  /** The `PUT` in flight was refused. */
  | { type: 'refused'; problem: ApiProblem }
  /** "Rewrite this file": the hand-written text may be replaced. */
  | { type: 'rewrite' }
  /** "Save as a new file": `doc` written under `name`, which must not exist. */
  | { type: 'saveAs'; doc: WireDocument; name: string }
  /** "Keep this pipeline". */
  | { type: 'keep' };

const json = (doc: WireDocument) => JSON.stringify(doc);

/** The state on opening `doc`; `rewrite` when "Rewrite this file" was chosen for it earlier this session. */
export function initialSave(doc: WireDocument, file: FileInit, rewrite: boolean): SaveState {
  return {
    file: { name: file.name, etag: file.etag, handwritten: file.name !== null && !file.canonical && !rewrite, proposed: file.proposed },
    saved: json(doc),
    keep: false,
    phase: { kind: 'idle' },
  };
}

const taken = (name: string) => `A pipeline named \`${name}\` already exists: choose another name.`;

export function saveReducer(state: SaveState, event: SaveEvent): SaveState {
  const { phase, file } = state;
  switch (event.type) {
    case 'valid': {
      // A write in flight, or a choice awaited, holds every other write back.
      if (phase.kind !== 'idle' && phase.kind !== 'failed') return state;
      const unchanged = json(event.doc) === state.saved;
      if (phase.kind === 'failed' && json(phase.doc) === json(event.doc)) return state;
      if (unchanged && !(file.name === null && state.keep)) return phase.kind === 'idle' ? state : { ...state, phase: { kind: 'idle' } };
      if (file.handwritten) return { ...state, phase: { kind: 'handwritten' } };
      const create = file.etag === null;
      return { ...state, phase: { kind: 'saving', doc: event.doc, name: file.name ?? file.proposed, create, back: null } };
    }
    case 'written':
      if (phase.kind !== 'saving') return state;
      return { file: { ...file, name: event.name, etag: event.etag, handwritten: false }, saved: json(event.doc), keep: false, phase: { kind: 'idle' } };
    case 'refused': {
      if (phase.kind !== 'saving') return state;
      const precondition = event.problem.code === 'precondition_failed';
      if (phase.back !== null) return { ...state, phase: { kind: phase.back, error: precondition ? taken(phase.name) : event.problem.message } };
      if (precondition) return { ...state, phase: phase.create ? { kind: 'taken', error: taken(phase.name) } : { kind: 'conflict' } };
      return { ...state, phase: { kind: 'failed', doc: phase.doc, problem: event.problem } };
    }
    case 'rewrite':
      if (phase.kind !== 'handwritten') return state;
      return { ...state, file: { ...file, handwritten: false }, phase: { kind: 'idle' } };
    case 'saveAs':
      if (phase.kind !== 'conflict' && phase.kind !== 'handwritten' && phase.kind !== 'taken') return state;
      return { ...state, phase: { kind: 'saving', doc: event.doc, name: event.name, create: true, back: phase.kind } };
    case 'keep':
      return file.name === null ? { ...state, keep: true } : state;
  }
}

/** Whether `doc` differs from what the editor last read or wrote. */
export const isDirty = (state: SaveState, doc: WireDocument) => json(doc) !== state.saved;
