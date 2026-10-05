import { describe, expect, it } from 'vitest';
import type { ApiProblem } from '../api/client.ts';
import type { WireDocument } from './document.ts';
import { HYBRID } from './fixtures.ts';
import { initialSave, saveReducer, type SaveState } from './saving.ts';

const EDITED: WireDocument = { pipeline: { ...HYBRID.pipeline, nodes: HYBRID.pipeline.nodes.slice(0, 1) } };
const OTHER: WireDocument = { pipeline: { ...HYBRID.pipeline, nodes: HYBRID.pipeline.nodes.slice(0, 2) } };
const ETAG = 'e'.repeat(64);

const problem = (code: ApiProblem['code'], message = 'refused'): ApiProblem => ({ code, message, hint: '', location: null, status: code === 'precondition_failed' ? 412 : 422 });

const stored = (canonical = true, rewrite = false): SaveState => initialSave(HYBRID, { name: 'hybrid', etag: ETAG, canonical, proposed: 'hybrid' }, rewrite);
const unnamed = (): SaveState => initialSave(HYBRID, { name: null, etag: null, canonical: true, proposed: 'example' }, false);

describe('the saving state machine', () => {
  it('writes nothing for the document it read', () => {
    const state = stored();
    expect(saveReducer(state, { type: 'valid', doc: HYBRID })).toBe(state);
  });

  it('writes a changed valid document over the etag it read', () => {
    const next = saveReducer(stored(), { type: 'valid', doc: EDITED });
    expect(next.phase).toEqual({ kind: 'saving', doc: EDITED, name: 'hybrid', create: false, back: null });
  });

  it('a write landing keeps its etag and makes its document the saved one', () => {
    const saving = saveReducer(stored(), { type: 'valid', doc: EDITED });
    const saved = saveReducer(saving, { type: 'written', doc: EDITED, name: 'hybrid', etag: 'f'.repeat(64) });
    expect(saved.phase).toEqual({ kind: 'idle' });
    expect(saved.file).toEqual({ name: 'hybrid', etag: 'f'.repeat(64), handwritten: false, proposed: 'hybrid' });
    expect(saveReducer(saved, { type: 'valid', doc: EDITED })).toBe(saved);
  });

  it('a newer valid document waits for the write in flight', () => {
    const saving = saveReducer(stored(), { type: 'valid', doc: EDITED });
    expect(saveReducer(saving, { type: 'valid', doc: OTHER })).toBe(saving);
  });

  it('a stale etag stops saving until a choice is made, and a valid document writes nothing meanwhile', () => {
    const saving = saveReducer(stored(), { type: 'valid', doc: EDITED });
    const conflict = saveReducer(saving, { type: 'refused', problem: problem('precondition_failed') });
    expect(conflict.phase).toEqual({ kind: 'conflict' });
    expect(saveReducer(conflict, { type: 'valid', doc: OTHER })).toBe(conflict);
  });

  it('keeps the canvas as a new file from a conflict, and goes on writing that file', () => {
    const conflict = saveReducer(saveReducer(stored(), { type: 'valid', doc: EDITED }), { type: 'refused', problem: problem('precondition_failed') });
    const saving = saveReducer(conflict, { type: 'saveAs', doc: EDITED, name: 'hybrid-2' });
    expect(saving.phase).toEqual({ kind: 'saving', doc: EDITED, name: 'hybrid-2', create: true, back: 'conflict' });
    const saved = saveReducer(saving, { type: 'written', doc: EDITED, name: 'hybrid-2', etag: 'a'.repeat(64) });
    expect(saved.file.name).toBe('hybrid-2');
    expect(saveReducer(saved, { type: 'valid', doc: OTHER }).phase).toEqual({ kind: 'saving', doc: OTHER, name: 'hybrid-2', create: false, back: null });
  });

  it('a new name already taken goes back to the choice, saying so', () => {
    const conflict = saveReducer(saveReducer(stored(), { type: 'valid', doc: EDITED }), { type: 'refused', problem: problem('precondition_failed') });
    const saving = saveReducer(conflict, { type: 'saveAs', doc: EDITED, name: 'taken' });
    expect(saveReducer(saving, { type: 'refused', problem: problem('precondition_failed') }).phase).toEqual({ kind: 'conflict', error: 'A pipeline named `taken` already exists: choose another name.' });
    expect(saveReducer(saving, { type: 'refused', problem: problem('request_invalid', 'not a name') }).phase).toEqual({ kind: 'conflict', error: 'not a name' });
  });

  it('a hand-written file asks before its first write, once', () => {
    const asking = saveReducer(stored(false), { type: 'valid', doc: EDITED });
    expect(asking.phase).toEqual({ kind: 'handwritten' });
    expect(saveReducer(asking, { type: 'valid', doc: OTHER })).toBe(asking);
    const rewriting = saveReducer(asking, { type: 'rewrite' });
    expect(rewriting.file.handwritten).toBe(false);
    expect(saveReducer(rewriting, { type: 'valid', doc: OTHER }).phase).toMatchObject({ kind: 'saving', name: 'hybrid', create: false });
  });

  it('a hand-written file whose rewrite was chosen this session asks nothing', () => {
    expect(saveReducer(stored(false, true), { type: 'valid', doc: EDITED }).phase).toMatchObject({ kind: 'saving' });
  });

  it('saving a hand-written file as a new one leaves it alone', () => {
    const asking = saveReducer(stored(false), { type: 'valid', doc: EDITED });
    expect(saveReducer(asking, { type: 'saveAs', doc: EDITED, name: 'mine' }).phase).toEqual({ kind: 'saving', doc: EDITED, name: 'mine', create: true, back: 'handwritten' });
  });

  it('an unnamed document is not written until its first edit, then asks for its name before it writes', () => {
    const state = unnamed();
    expect(saveReducer(state, { type: 'valid', doc: HYBRID })).toBe(state);
    const naming = saveReducer(state, { type: 'valid', doc: EDITED });
    expect(naming.phase).toEqual({ kind: 'naming' });
    // Changes made while the question is open are written once it is answered, not before.
    expect(saveReducer(naming, { type: 'valid', doc: OTHER })).toBe(naming);
    expect(saveReducer(naming, { type: 'saveAs', doc: OTHER, name: 'mine' }).phase).toEqual({ kind: 'saving', doc: OTHER, name: 'mine', create: true, back: 'naming' });
  });

  it('“Keep this pipeline” asks for the name of an unedited document', () => {
    const kept = saveReducer(unnamed(), { type: 'keep' });
    expect(saveReducer(kept, { type: 'valid', doc: HYBRID }).phase).toEqual({ kind: 'naming' });
  });

  it('a name taken asks again, saying so', () => {
    const naming = saveReducer(unnamed(), { type: 'valid', doc: EDITED });
    const saving = saveReducer(naming, { type: 'saveAs', doc: EDITED, name: 'example' });
    expect(saveReducer(saving, { type: 'refused', problem: problem('precondition_failed') }).phase).toEqual({ kind: 'naming', error: 'A pipeline named `example` already exists: choose another name.' });
  });

  it('a name proposed before the first write is the one the question offers', () => {
    expect(saveReducer(unnamed(), { type: 'propose', name: 'lexical-only' }).file.proposed).toBe('lexical-only');
    // A file already written is renamed, not proposed.
    const state = stored();
    expect(saveReducer(state, { type: 'propose', name: 'other' })).toBe(state);
  });

  it('a rename keeps the etag, since the bytes moved as they are, and writes on under the new name', () => {
    const renamed = saveReducer(stored(), { type: 'renamed', name: 'lexical-only' });
    expect(renamed.file).toEqual({ name: 'lexical-only', etag: ETAG, handwritten: false, proposed: 'hybrid' });
    expect(saveReducer(renamed, { type: 'valid', doc: EDITED }).phase).toEqual({ kind: 'saving', doc: EDITED, name: 'lexical-only', create: false, back: null });
  });

  it('a write the server refuses otherwise is said, and not retried until the document changes', () => {
    const saving = saveReducer(stored(), { type: 'valid', doc: EDITED });
    const failed = saveReducer(saving, { type: 'refused', problem: problem('pipeline_invalid', 'a key no component reads') });
    expect(failed.phase).toEqual({ kind: 'failed', doc: EDITED, problem: problem('pipeline_invalid', 'a key no component reads') });
    expect(saveReducer(failed, { type: 'valid', doc: EDITED })).toBe(failed);
    expect(saveReducer(failed, { type: 'valid', doc: OTHER }).phase).toMatchObject({ kind: 'saving', doc: OTHER });
  });
});
