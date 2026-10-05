// "Fork this run", from Runs and from Replay: the run's own configuration —
// every run keeps it, verbatim — written byte for byte to a new pipeline file
// under a proposed name (sent as text, ADR-C40 § 6), the layout copied at the
// run's launch copied beside it when there is one, and the editor opened on it
// with the run named in its header — on the node selected in Replay, when it
// is forked from there. The iterate journey's first click (design
// document § 3). ARCHITECTURE.md § The editor.
import { useState } from 'react';
import { Button, ButtonLink, InlineMessage } from '../../design/index.ts';
import type { ApiClient, ApiProblem } from '../api/client.ts';
import { formatHash, navigate } from '../routes.ts';
import { freshName } from './example.ts';
import { rememberFork } from './session.ts';

/** How many names a fork tries when each it proposes is taken before it is written. */
const ATTEMPTS = 5;

export type Forked =
  | { ok: true; name: string; layoutFailed: ApiProblem | null }
  | { ok: false; problem: ApiProblem };

/** Forks `run`: the new pipeline's name, and why its layout was not copied, if it was not; or why nothing was written. */
export async function forkRun(client: ApiClient, run: string): Promise<Forked> {
  const detail = await client.get('/runs/{id}', { id: run });
  if (!detail.ok) return detail;
  const listing = await client.get('/pipelines');
  if (!listing.ok) return listing;
  const base = `${detail.value.launched_as?.name ?? `run-${run.slice(0, 8)}`}-fork`;
  const taken = listing.value.pipelines.map((p) => p.name);
  // A name taken between the listing and the write — by another tab, another
  // editor — is skipped for the next one free; the write itself never replaces.
  let written = null;
  for (let attempt = 0; attempt < ATTEMPTS; attempt += 1) {
    const name = freshName(base, taken);
    const result = await client.put('/pipelines/{name}', { document: detail.value.configuration }, { name }, { headers: { 'If-None-Match': '*' } });
    if (result.ok) {
      written = result.value.name;
      break;
    }
    if (result.problem.code !== 'precondition_failed' || attempt === ATTEMPTS - 1) return result;
    taken.push(name);
  }
  if (written === null) return { ok: false, problem: { code: 'precondition_failed', message: 'Every name proposed for the fork was taken.', hint: 'Fork it again.', location: null, status: 412 } };
  rememberFork(written, run);
  // The positions are presentation: a fork whose layout cannot be copied is
  // still a fork, laid out by the canvas — and says so.
  const layout = await client.get('/runs/{id}/layout', { id: run });
  if (!layout.ok) return { ok: true, name: written, layoutFailed: layout.problem };
  if (layout.value.layout === null) return { ok: true, name: written, layoutFailed: null };
  const copied = await client.put('/pipelines/{name}/layout', layout.value.layout, { name: written });
  return { ok: true, name: written, layoutFailed: copied.ok ? null : copied.problem };
}

/**
 * "Fork this run"; refused, saying `refusal`, while there is no one run to
 * fork. Given the node selected where it is pressed — Replay's — it opens the
 * fork on that node: the fork is the run's own configuration, so it has it.
 */
export function ForkButton({ client, run, size = 'm', refusal = null, node = null }: { client: ApiClient; run: string | null; size?: 's' | 'm'; refusal?: string | null; node?: string | null }) {
  const editorAt = (name: string) => (node === null ? { screen: 'editor' as const, name } : { screen: 'editor' as const, name, node });
  const [busy, setBusy] = useState(false);
  const [refused, setRefused] = useState<ApiProblem | null>(null);
  const [partly, setPartly] = useState<{ name: string; problem: ApiProblem } | null>(null);
  const fork = async () => {
    if (run === null) return;
    setBusy(true);
    setRefused(null);
    setPartly(null);
    const forked = await forkRun(client, run);
    setBusy(false);
    if (!forked.ok) setRefused(forked.problem);
    else if (forked.layoutFailed !== null) setPartly({ name: forked.name, problem: forked.layoutFailed });
    else navigate(editorAt(forked.name));
  };
  return (
    <>
      <Button size={size} busy={busy} busyLabel="Forking…" onClick={() => void fork()} {...(run === null || refusal !== null ? { disabled: true, disabledReason: refusal ?? 'There is no run to fork.' } : {})}>
        Fork this run
      </Button>
      {refused === null ? null : (
        <InlineMessage tone="critical" title="The run could not be forked. Nothing was opened.">
          {refused.message}
        </InlineMessage>
      )}
      {partly === null ? null : (
        <InlineMessage
          tone="warning"
          title={`Forked as ${partly.name}, but its layout could not be copied: ${partly.problem.message}`}
          action={
            <ButtonLink size="s" href={formatHash(editorAt(partly.name))}>
              Open {partly.name} in the editor
            </ButtonLink>
          }
        >
          The canvas lays it out itself.
        </InlineMessage>
      )}
    </>
  );
}
