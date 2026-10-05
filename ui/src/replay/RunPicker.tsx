// Replay before a run is chosen (`#replay`): the workspace's runs to pick
// one from, and the one action, opening it — or, with none, the way to Runs.
// It carries no canvas, so the shell imports it without Replay's chunk.
// ARCHITECTURE.md § The Replay screen.
import { useCallback, useEffect, useState } from 'react';
import { ButtonLink, EmptyState, Select, Sheet } from '../../design/index.ts';
import type { ApiClient } from '../api/client.ts';
import type { RunListing } from '../api/types.ts';
import { formatHash } from '../routes.ts';
import { prefixText } from '../runs/model.ts';
import { ErrorState, Loading, type RequestState } from '../shell/states.tsx';
import { runName } from './model.ts';
import './Replay.css';

export function RunPicker({ client }: { client: ApiClient }) {
  const [listing, setListing] = useState<RequestState<RunListing>>({ status: 'loading' });
  const [chosen, setChosen] = useState<string | null>(null);
  const read = useCallback(
    (signal?: AbortSignal) => {
      setListing({ status: 'loading' });
      void client.get('/runs', signal === undefined ? {} : { signal }).then((result) => {
        if (signal?.aborted === true) return;
        setListing(result.ok ? { status: 'loaded', value: result.value } : { status: 'error', problem: result.problem });
      });
    },
    [client],
  );
  useEffect(() => {
    const controller = new AbortController();
    read(controller.signal);
    return () => controller.abort();
  }, [read]);

  if (listing.status === 'loading') {
    return (
      <Sheet>
        <Loading label="Reading the runs" />
      </Sheet>
    );
  }
  if (listing.status === 'error') return <ErrorState problem={listing.problem} onRetry={() => read()} />;
  const { runs } = listing.value;
  if (runs.length === 0) {
    return (
      <Sheet>
        <EmptyState
          heading="No run to replay yet"
          action={
            <ButtonLink kind="primary" size="l" href={formatHash({ screen: 'runs' })}>
              Open Runs
            </ButtonLink>
          }
        >
          Launch a pipeline from Runs; once it has run, follow any of its queries through the pipeline here, node by node.
        </EmptyState>
      </Sheet>
    );
  }
  const run = runs.find((r) => r.id === chosen) ?? runs[0]!;
  return (
    <Sheet>
      <EmptyState
        heading="Choose a run to replay"
        action={
          <div className="rg-replay__pick">
            <Select
              id="replay-run"
              label="Run"
              value={run.id}
              onChange={(e) => setChosen(e.target.value)}
              options={runs.map((r) => {
                const prefix = prefixText(r);
                return { value: r.id, label: [runName(r) ?? 'run', r.id.slice(0, 12), r.benchmark_names[0] ?? r.dataset_version.slice(0, 12), ...(prefix === null ? [] : [prefix])].join(' · ') };
              })}
            />
            <ButtonLink kind="primary" size="l" icon="play" href={formatHash({ screen: 'replay', run: run.id })}>
              Replay this run
            </ButtonLink>
          </div>
        }
      >
        Follow one of its queries through the pipeline, node by node; or open a query from Compare to replay it beside another run.
      </EmptyState>
    </Sheet>
  );
}
