// The Setup screen's two forms: importing a local corpus, and connecting a
// service. Each submits through a callback that answers with the API's
// refusal or null, and shows a refusal under its field in the API's own
// words — inline, where it is corrected (the front-end design, § 8).
import { useEffect, useId, useState, type FormEvent } from 'react';
import { Button, InlineMessage, Input, Select } from '../../design/index.ts';
import type { ApiProblem } from '../api/client.ts';

/** A refusal as the line under a field: the API's detail, then its hint. */
const words = (problem: ApiProblem) => `${problem.message} ${problem.hint}`;

/**
 * Moves focus to the field a refusal is shown under, each time one arrives,
 * so a screen reader reads its description — the API's words.
 */
function useFocusOnRefusal(refused: ApiProblem | null, field: string) {
  useEffect(() => {
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
          help="A directory on the server’s disk holding the corpus and its ground truth."
          {...(refused === null ? {} : { error: words(refused) })}
        />
        <Input id={`${id}-name`} label="Import as" value={name} onChange={(e) => setName(e.target.value)} help="One directory name: letters, digits, “.”, “_” and “-”." />
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

export type ConnectFormProps = {
  /** The families a binding can name, as `GET /workspace` lists them; a free field when none are known. */
  families: readonly string[];
  /** The family chosen first. */
  initialFamily?: string;
  /** Whether this build carries `remote`; without it a binding is stored and every test of it refused. */
  remote: boolean | null;
  /** Binds and then probes; resolves to the write's refusal, or null once the binding is stored. */
  onConnect: (binding: { family: string; name: string; uri: string; servedModel: string }) => Promise<ApiProblem | null>;
};

/** "Connect": a family, a name and an address, and the served model the identity read needs. */
export function ConnectForm({ families, initialFamily, remote, onConnect }: ConnectFormProps) {
  const id = useId();
  // What the person chose, if anything. The family sent is derived from the
  // families known now, since `GET /workspace` may answer after the form is drawn.
  const [chosen, setChosen] = useState('');
  const family = families.length === 0 ? chosen : families.includes(chosen) ? chosen : initialFamily !== undefined && families.includes(initialFamily) ? initialFamily : (families[0] as string);
  const setFamily = setChosen;
  const [name, setName] = useState('');
  const [uri, setUri] = useState('');
  const [servedModel, setServedModel] = useState('');
  const [busy, setBusy] = useState(false);
  const [refused, setRefused] = useState<ApiProblem | null>(null);
  const [said, setSaid] = useState<string | null>(null);
  useFocusOnRefusal(refused, `${id}-uri`);

  const submit = async (event: FormEvent) => {
    event.preventDefault();
    if (busy) return;
    setBusy(true);
    setSaid(`Connecting ${family}/${name}…`);
    const problem = await onConnect({ family, name, uri, servedModel });
    setBusy(false);
    setRefused(problem);
    setSaid(null);
    if (problem === null) {
      setName('');
      setUri('');
      setServedModel('');
    }
  };

  return (
    <form className="rg-setup__form" aria-label="Connect a service" onSubmit={(e) => void submit(e)}>
      {remote === false ? (
        <InlineMessage tone="info" title="This build has no remote feature.">
          A binding is stored in workspace.toml, and testing it is refused until the binary is built with the remote feature.
        </InlineMessage>
      ) : null}
      <div className="rg-setup__fields">
        {families.length === 0 ? (
          <Input id={`${id}-family`} label="Family" value={family} onChange={(e) => setFamily(e.target.value)} />
        ) : (
          <Select id={`${id}-family`} label="Family" value={family} onChange={(e) => setFamily(e.target.value)} options={families.map((f) => ({ value: f, label: f }))} />
        )}
        <Input id={`${id}-name`} label="Name" value={name} onChange={(e) => setName(e.target.value)} help="The implementation name a node uses." />
        <Input
          id={`${id}-uri`}
          label="Address"
          mono
          value={uri}
          onChange={(e) => setUri(e.target.value)}
          help="The scheme http, a host and an optional port, as ragondin bench --remote takes it."
          {...(refused === null ? {} : { error: words(refused) })}
        />
        <Input id={`${id}-model`} label="Served model" mono value={servedModel} onChange={(e) => setServedModel(e.target.value)} help="The model the service serves, as a node’s served_model; an embedder, a reranker or a generator needs one to report an identity." />
      </div>
      <div className="rg-setup__submit">
        <Button type="submit" kind="primary" busy={busy}>
          Connect
        </Button>
        <span role="status" className="rg-setup__said">
          {said}
        </span>
      </div>
    </form>
  );
}
