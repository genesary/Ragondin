// "Fork this run", from Runs and from Replay: the run's own configuration —
// every run keeps it, verbatim — written byte for byte to a new pipeline file
// under a proposed name (sent as text, ADR-C40 § 6), the layout copied at the
// run's launch copied beside it when there is one, and the editor opened on it
// with the run named in its header. The iterate journey's first click (design
// document § 3). ARCHITECTURE.md § The editor.
import { useState } from 'react';
import { Button, InlineMessage } from '../../design/index.ts';
import type { ApiClient, ApiProblem } from '../api/client.ts';
import { navigate } from '../routes.ts';
import { freshName } from './example.ts';
import { rememberFork } from './session.ts';

/** Forks `run`, answering the new pipeline's name, or why it could not. */
export async function forkRun(client: ApiClient, run: string): Promise<{ ok: true; name: string } | { ok: false; problem: ApiProblem }> {
  const detail = await client.get('/runs/{id}', { id: run });
  if (!detail.ok) return detail;
  const listing = await client.get('/pipelines');
  if (!listing.ok) return listing;
  const base = `${detail.value.launched_as?.name ?? `run-${run.slice(0, 8)}`}-fork`;
  const name = freshName(base, listing.value.pipelines.map((p) => p.name));
  const written = await client.put('/pipelines/{name}', { document: detail.value.configuration }, { name }, { headers: { 'If-None-Match': '*' } });
  if (!written.ok) return written;
  // The positions are presentation: a fork whose layout cannot be copied
  // still opens, laid out by the canvas.
  const layout = await client.get('/runs/{id}/layout', { id: run });
  if (layout.ok && layout.value.layout !== null) await client.put('/pipelines/{name}/layout', layout.value.layout, { name: written.value.name });
  rememberFork(written.value.name, run);
  return { ok: true, name: written.value.name };
}

/** "Fork this run"; refused, saying `refusal`, while there is no one run to fork. */
export function ForkButton({ client, run, size = 'm', refusal = null }: { client: ApiClient; run: string | null; size?: 's' | 'm'; refusal?: string | null }) {
  const [busy, setBusy] = useState(false);
  const [refused, setRefused] = useState<ApiProblem | null>(null);
  const fork = async () => {
    if (run === null) return;
    setBusy(true);
    setRefused(null);
    const forked = await forkRun(client, run);
    setBusy(false);
    if (forked.ok) navigate({ screen: 'editor', name: forked.name });
    else setRefused(forked.problem);
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
    </>
  );
}
