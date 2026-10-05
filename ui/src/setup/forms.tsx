// The Setup screen's two forms: importing a local corpus, and connecting a
// service. Each submits through a callback that answers with the API's
// refusal or null, and shows a refusal under its field in the API's own
// words — inline, where it is corrected (the front-end design, § 8).
import { useId, useLayoutEffect, useState, type FormEvent } from 'react';
import { Button, InlineMessage, Input, Select } from '../../design/index.ts';
import type { ApiProblem } from '../api/client.ts';
import { familyLabel } from './model.ts';

/** A refusal as the line under a field: the API's detail, then its hint. */
const words = (problem: ApiProblem) => `${problem.message} ${problem.hint}`;

/**
 * Moves focus to the field a refusal is shown under, each time one arrives,
 * so a screen reader reads its description — the API's words. A layout
 * effect, so focus moves in the same commit that shows the refusal: a passive
 * effect runs later, and anything reading the page in between — a screen
 * reader, or a test waiting on the field's `aria-invalid` — finds the words
 * there and focus not yet on them.
 */
function useFocusOnRefusal(refused: ApiProblem | null, field: string) {
  useLayoutEffect(() => {
    if (refused !== null) document.getElementById(field)?.focus();
  }, [refused, field]);
}

export type ImportFormProps = {
  /** Imports `path` as `name`; resolves to the refusal, or null once the benchmark is listed. */
  onImport: (path: string, name: string) => Promise<ApiProblem | null>;
};

/** "Import a local corpus": a directory on the server's disk, and the name to list it under. */
export function ImportForm({ onImport }: ImportFormProps) {
  const id = useId();
  const [path, setPath] = useState('');
  const [name, setName] = useState('');
  const [busy, setBusy] = useState(false);
  const [refused, setRefused] = useState<ApiProblem | null>(null);
  const [done, setDone] = useState<string | null>(null);
  useFocusOnRefusal(refused, `${id}-path`);

  const submit = async (event: FormEvent) => {
    event.preventDefault();
    if (busy) return;
    setBusy(true);
    setDone(`Importing ${name}…`);
    const problem = await onImport(path, name);
    setBusy(false);
    setRefused(problem);
    setDone(problem === null ? `Imported ${name}.` : null);
    if (problem === null) {
      setPath('');
      setName('');
    }
  };

  return (
    <form className="rg-setup__form" aria-label="Import a local corpus" onSubmit={(e) => void submit(e)}>
      <div className="rg-setup__fields">
        <Input
          id={`${id}-path`}
          label="Corpus directory"
          mono
          value={path}
          onChange={(e) => setPath(e.target.value)}
          placeholder="/data/my-corpus"
          help="A directory on the server’s disk holding the corpus and its ground truth."
          {...(refused === null ? {} : { error: words(refused) })}
        />
        <Input id={`${id}-name`} label="Import as" placeholder="my-corpus" value={name} onChange={(e) => setName(e.target.value)} help="One directory name: letters, digits, “.”, “_” and “-”." />
      </div>
      <div className="rg-setup__submit">
        {/* The button keeps its word while busy, so nothing beside it moves; the line says what is happening. */}
        <Button type="submit" busy={busy}>
          Import
        </Button>
        {/* Always present, so the line that says what happened moves nothing when it fills. */}
        <span role="status" className="rg-setup__said">
          {done}
        </span>
      </div>
    </form>
  );
}

/** What the Connect form holds, kept by its caller so it survives the form being drawn again elsewhere — the first launch ending. */
export type ConnectDraft = { family: string; name: string; uri: string; servedModel: string };

export const EMPTY_DRAFT: ConnectDraft = { family: '', name: '', uri: '', servedModel: '' };

export type ConnectFormProps = {
  /** The families a binding can name, as `GET /workspace` lists them; a free field when none are known. */
  families: readonly string[];
  /** The family chosen first. */
  initialFamily?: string;
  /** Whether this build carries `remote`; without it the form is refused, saying why. */
  remote: boolean | null;
  /** What the form holds; `family` is what the person chose, empty before they chose. */
  draft: ConnectDraft;
  onDraft: (draft: ConnectDraft) => void;
  /** Binds and then probes; resolves to the write's refusal, or null once the binding is stored. */
  onConnect: (binding: { family: string; name: string; uri: string; servedModel: string }) => Promise<ApiProblem | null>;
};

const NO_REMOTE = 'This build cannot call a service: it was built without the remote feature.';

/** "Connect": a family, a name and an address, and the served model the identity read needs. */
export function ConnectForm({ families, initialFamily, remote, draft, onDraft, onConnect }: ConnectFormProps) {
  const id = useId();
  // The family sent is derived from the families known now, since
  // `GET /workspace` may answer after the form is drawn.
  const chosen = draft.family;
  const family = families.length === 0 ? chosen : families.includes(chosen) ? chosen : initialFamily !== undefined && families.includes(initialFamily) ? initialFamily : (families[0] as string);
  const { name, uri, servedModel } = draft;
  const set = (change: Partial<ConnectDraft>) => onDraft({ ...draft, ...change });
  const [busy, setBusy] = useState(false);
  const [refused, setRefused] = useState<ApiProblem | null>(null);
  const [said, setSaid] = useState<string | null>(null);
  useFocusOnRefusal(refused, `${id}-uri`);
  const off = remote === false;

  const submit = async (event: FormEvent) => {
    event.preventDefault();
    if (busy || off) return;
    setBusy(true);
    setSaid(`Connecting ${family}/${name}…`);
    const problem = await onConnect({ family, name, uri, servedModel });
    setBusy(false);
    setRefused(problem);
    setSaid(null);
    if (problem === null) onDraft({ ...draft, family, name: '', uri: '', servedModel: '' });
  };

  return (
    <form className="rg-setup__form" aria-label="Connect a service" onSubmit={(e) => void submit(e)}>
      {off ? (
        <InlineMessage tone="info" title="This build has no remote feature.">
          It cannot call a service, so connecting one is refused here. A pipeline runs on the components built in; rebuild with the remote feature to call a model server.
        </InlineMessage>
      ) : null}
      <div className="rg-setup__fields">
        {families.length === 0 ? (
          <Input id={`${id}-family`} label="Family" placeholder="generator" disabled={off} value={family} onChange={(e) => set({ family: e.target.value })} />
        ) : (
          <Select id={`${id}-family`} label="Family" disabled={off} value={family} onChange={(e) => set({ family: e.target.value })} options={families.map((f) => ({ value: f, label: familyLabel(f) }))} />
        )}
        <Input id={`${id}-name`} label="Name" placeholder="qwen" disabled={off} value={name} onChange={(e) => set({ name: e.target.value })} help="The implementation name a node uses." />
        <Input
          id={`${id}-uri`}
          label="Address"
          mono
          disabled={off}
          value={uri}
          onChange={(e) => set({ uri: e.target.value })}
          help="The scheme http, then a host and an optional port: where the model server listens."
          {...(refused === null ? {} : { error: words(refused) })}
        />
        <Input id={`${id}-model`} label="Served model" mono placeholder="qwen2.5-7b-instruct" disabled={off} value={servedModel} onChange={(e) => set({ servedModel: e.target.value })} help="The model the server serves; an embedder, a reranker or a generator needs it to say which model answered." />
      </div>
      <div className="rg-setup__submit">
        {off ? (
          <Button type="submit" kind="primary" disabled disabledReason={NO_REMOTE}>
            Connect
          </Button>
        ) : (
          <Button type="submit" kind="primary" busy={busy}>
            Connect
          </Button>
        )}
        <span role="status" className="rg-setup__said">
          {said}
        </span>
      </div>
    </form>
  );
}
