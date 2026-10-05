// What the editor says when saving stops for a choice, and the export. The
// conflict and the hand-written-file warning are the editor's two prompts —
// the second is the application's second confirmation dialog (design
// document § 3) — and they are inline messages beside the editor's bar, never
// a modal: the canvas stays usable behind them, and nothing is written until
// one of their actions is taken. ARCHITECTURE.md § The editor.
import { useId, useState, type FormEvent } from 'react';
import { Button, InlineMessage, Input } from '../../design/index.ts';
import type { Phase } from './saving.ts';

/**
 * Everything the server's rendering does not keep of a text a person wrote,
 * named in full (ADR-C40 § 7).
 */
export const DROPPED =
  'its comments, its formatting, its key order, expanded references (an anchor and its aliases are written out in full at each use), and keys the pipeline schema does not read';

function NewName({ proposed, error, onSave }: { proposed: string; error: string | undefined; onSave: (name: string) => void }) {
  const id = useId();
  const [name, setName] = useState(proposed);
  const submit = (event: FormEvent) => {
    event.preventDefault();
    if (name.trim() !== '') onSave(name.trim());
  };
  return (
    <form className="rg-editor__new-name" onSubmit={submit}>
      <Input id={`${id}-name`} label="New file name" mono value={name} onChange={(e) => setName(e.target.value)} {...(error === undefined ? {} : { error })} />
      <Button type="submit" {...(name.trim() === '' ? { disabled: true, disabledReason: 'Give the new file a name.' } : {})}>
        Save as a new file
      </Button>
    </form>
  );
}

export type SavePromptProps = {
  phase: Phase;
  /** The file the editor writes, or the name proposed for it. */
  name: string;
  onReload: () => void;
  onRewrite: () => void;
  onSaveAs: (name: string) => void;
};

/** The prompt a phase waits on, or nothing. */
export function SavePrompt({ phase, name, onReload, onRewrite, onSaveAs }: SavePromptProps) {
  switch (phase.kind) {
    case 'conflict':
      return (
        <section className="rg-editor__prompt" aria-label="The file changed on disk">
          <InlineMessage
            tone="warning"
            title={`${name}.yaml changed on disk since the editor read it. Nothing was written.`}
            action={
              <Button kind="quiet" onClick={onReload}>
                Discard my changes and reload
              </Button>
            }
          >
            Reload it from disk, and your unsaved changes on the canvas are lost; or keep the canvas’s version as a new file, and {name}.yaml stays as it is on disk.
          </InlineMessage>
          <NewName proposed={`${name}-mine`} error={phase.error} onSave={onSaveAs} />
        </section>
      );
    case 'handwritten':
      return (
        <section className="rg-editor__prompt" aria-label="This file was written by hand">
          <InlineMessage
            tone="warning"
            title={`${name}.yaml was written by hand. Nothing is written until you choose.`}
            action={<Button onClick={onRewrite}>Rewrite this file</Button>}
          >
            Saving from the canvas rewrites it as the server renders it, and drops what that rendering does not keep: {DROPPED}. Or save the canvas as a new file, and {name}.yaml stays as it is.
          </InlineMessage>
          <NewName proposed={`${name}-canvas`} error={phase.error} onSave={onSaveAs} />
        </section>
      );
    case 'taken':
      return (
        <section className="rg-editor__prompt" aria-label="This name is taken">
          <InlineMessage tone="warning" title={`A pipeline named ${name} already exists. Nothing was written.`}>
            Give this pipeline another name to write it.
          </InlineMessage>
          <NewName proposed={`${name}-2`} error={phase.error} onSave={onSaveAs} />
        </section>
      );
    default:
      return null;
  }
}

/**
 * The export: the server's rendering of the document as it stands — the bytes
 * a write of it stores, as `POST /pipelines/validate` answered them — to copy,
 * or to save as a file. Nothing of the browser's own is added.
 */
export function ExportPanel({ name, rendering }: { name: string; rendering: string }) {
  const id = useId();
  const [copied, setCopied] = useState('');
  const copy = () => {
    void navigator.clipboard?.writeText(rendering).then(
      () => setCopied('Copied.'),
      () => setCopied('The browser refused the copy: select the text and copy it.'),
    );
  };
  const download = () => {
    const url = URL.createObjectURL(new Blob([rendering], { type: 'application/yaml' }));
    const link = document.createElement('a');
    link.href = url;
    link.download = `${name}.yaml`;
    link.click();
    URL.revokeObjectURL(url);
  };
  return (
    <section className="rg-editor__export" aria-label="Export">
      <label className="rg-field__label" htmlFor={`${id}-text`}>
        The document as the server renders it
      </label>
      <textarea id={`${id}-text`} className="rg-editor__export-text" readOnly value={rendering} rows={Math.min(16, rendering.split('\n').length)} />
      <div className="rg-editor__export-actions">
        <Button size="s" onClick={copy}>
          Copy
        </Button>
        <Button size="s" onClick={download}>
          Download {name}.yaml
        </Button>
        <span role="status">{copied}</span>
      </div>
    </section>
  );
}
