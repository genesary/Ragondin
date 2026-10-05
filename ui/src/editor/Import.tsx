// Import: a pipeline document pasted or chosen from disk, checked as
// `ragondin validate` checks a file — sent as text, which the server reads
// itself (ADR-C40 § 6) — its error shown where it is when it does not
// validate, and on success written byte for byte as a new pipeline file under
// the name the person gives. ARCHITECTURE.md § The editor.
import { useId, useState, type ChangeEvent, type FormEvent } from 'react';
import { Button, InlineMessage, Input, Section } from '../../design/index.ts';
import type { ApiClient, ApiProblem } from '../api/client.ts';
import { Words } from '../words.tsx';

/** Where a refusal is, in words: the node, and the edge when the server names one. */
export function locationWords(problem: ApiProblem): string {
  const at = problem.location;
  if (at === null || at.node === null) return '';
  const edge = at.edge === null ? '' : `, on the edge \`${at.edge.from}\` → \`${at.edge.to}\`, port ${at.edge.port}`;
  return `At node \`${at.node}\`${edge}.`;
}

// The prefix the server's refusal of a document starts with (`ApiError::PipelineInvalid`): the title already says
// it, so the detail is shown without it.
const INVALID_PREFIX = 'the pipeline does not validate: ';
const withoutPrefix = (message: string) => (message.startsWith(INVALID_PREFIX) ? message.slice(INVALID_PREFIX.length) : message);

// A file name's stem as a pipeline name: what the server's name rule keeps.
const stem = (file: string) => file.replace(/\.(ya?ml)$/i, '').replace(/[^A-Za-z0-9._-]/g, '-').slice(0, 64);

export type ImportPanelProps = {
  client: ApiClient;
  /** The pipeline was written under `name`. */
  onImported: (name: string) => void;
  onCancel: () => void;
};

export function ImportPanel({ client, onImported, onCancel }: ImportPanelProps) {
  const id = useId();
  const [text, setText] = useState('');
  const [name, setName] = useState('');
  // Whether the person typed the name: until then it follows the file chosen.
  const [named, setNamed] = useState(false);
  const [chosen, setChosen] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [refused, setRefused] = useState<{ title: string; detail: string } | null>(null);

  const choose = (event: ChangeEvent<HTMLInputElement>) => {
    const file = event.target.files?.[0];
    if (file === undefined) return;
    setChosen(file.name);
    void file.text().then((read) => {
      setText(read);
      if (!named) setName(stem(file.name));
    });
  };

  const submit = async (event: FormEvent) => {
    event.preventDefault();
    setBusy(true);
    setRefused(null);
    const checked = await client.post('/pipelines/validate', { document: text });
    if (!checked.ok) {
      setBusy(false);
      setRefused({ title: 'This document is not a valid pipeline. Nothing was written.', detail: [withoutPrefix(checked.problem.message), locationWords(checked.problem)].filter(Boolean).join(' ') });
      return;
    }
    const written = await client.put('/pipelines/{name}', { document: text }, { name }, { headers: { 'If-None-Match': '*' } });
    setBusy(false);
    if (!written.ok) {
      const taken = written.problem.code === 'precondition_failed';
      setRefused({
        title: taken ? `A pipeline named \`${name}\` already exists. Nothing was written.` : 'The pipeline could not be written.',
        detail: taken ? 'Give this one another name.' : [withoutPrefix(written.problem.message), locationWords(written.problem)].filter(Boolean).join(' '),
      });
      return;
    }
    onImported(written.value.name);
  };

  const why = text.trim() === '' ? 'Paste a pipeline document or choose a file.' : name.trim() === '' ? 'Give the pipeline a name.' : null;
  return (
    <Section heading="Import a pipeline" caption="Checked as ragondin validate checks a file, then written as it is, comments and all.">
      <form className="rg-editor__import" onSubmit={(e) => void submit(e)}>
        <div className="rg-editor__import-file">
          {/* The native control is kept for the keyboard and the file dialog, and drawn as one of the design's buttons. */}
          <input id={`${id}-file`} className="rg-visually-hidden" type="file" accept=".yaml,.yml,application/yaml,text/yaml" onChange={choose} />
          <label className="rg-btn rg-btn--secondary" htmlFor={`${id}-file`}>
            Choose a YAML file…
          </label>
          <span className="rg-editor__import-chosen">{chosen ?? 'No file chosen'}</span>
        </div>
        <div className="rg-field">
          <label className="rg-field__label" htmlFor={`${id}-text`}>
            Pipeline document (YAML)
          </label>
          <textarea id={`${id}-text`} className="rg-editor__import-text" rows={12} spellCheck={false} value={text} onChange={(e) => setText(e.target.value)} />
        </div>
        <Input
          id={`${id}-name`}
          label="Pipeline name"
          mono
          value={name}
          onChange={(e) => {
            setName(e.target.value);
            setNamed(true);
          }}
          help="Written as pipelines/<name>.yaml."
        />
        {refused === null ? null : (
          <InlineMessage tone="critical" title={<Words text={refused.title} />}>
            <Words text={refused.detail} />
          </InlineMessage>
        )}
        <div className="rg-editor__import-actions">
          <Button kind="primary" type="submit" busy={busy} busyLabel="Importing…" {...(why === null ? {} : { disabled: true, disabledReason: why })}>
            Validate and import
          </Button>
          <Button kind="quiet" onClick={onCancel}>
            Cancel
          </Button>
          {/* Seen beside the button; the button itself says it to assistive technology. */}
          {why === null ? null : (
            <span className="rg-editor__import-why" aria-hidden="true">
              {why}
            </span>
          )}
        </div>
      </form>
    </Section>
  );
}
