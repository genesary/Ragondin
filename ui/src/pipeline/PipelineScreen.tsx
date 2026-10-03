// The Pipeline screen: one pipeline, all its runs, as a matrix of node ×
// benchmark read from one `GET /pipelines/{name}/matrix` (the front-end
// design, § 3). Its state is the address, `#pipeline/<name>`; it owns the
// ranking metric the rows read and nothing else. ARCHITECTURE.md § The
// Pipeline screen.
import { useCallback, useEffect, useRef, useState } from 'react';
import { Button, ButtonLink, EmptyState, FAMILY_LABEL, FamilyTile, InlineMessage, Section, Select, Sheet, familyOfComponent } from '../../design/index.ts';
import type { ApiClient } from '../api/client.ts';
import type { PipelineListing, PipelineMatrix } from '../api/types.ts';
import { formatHash, navigate } from '../routes.ts';
import { shortHash } from '../runs/model.ts';
import { ErrorState, Loading, type RequestState } from '../shell/states.tsx';
import { FeedingRuns } from './FeedingRuns.tsx';
import { rememberPipeline } from './last.ts';
import { Matrix, NO_LAUNCHER, type Launch } from './Matrix.tsx';
import { columnLabel, missingCount, rankingMetrics, verdict } from './model.ts';
import './Pipeline.css';

export type PipelineScreenProps = {
  client: ApiClient;
  /** The pipeline the address names, if it names one. */
  name?: string | undefined;
  /** The launcher's entry point, once it exists; every Run is refused with its reason until then. */
  launch?: Launch | undefined;
};

const runs = (n: number) => `${n.toLocaleString('en-US')} run${n === 1 ? '' : 's'}`;

/** The workspace's pipelines, read once for the selector; a failure leaves the selector with the current name alone. */
function usePipelines(client: ApiClient): RequestState<PipelineListing> {
  const [listing, setListing] = useState<RequestState<PipelineListing>>({ status: 'loading' });
  useEffect(() => {
    let live = true;
    void client.get('/pipelines').then((result) => {
      if (live) setListing(result.ok ? { status: 'loaded', value: result.value } : { status: 'error', problem: result.problem });
    });
    return () => {
      live = false;
    };
  }, [client]);
  return listing;
}

/** The pipeline selector: every pipeline of the workspace by name; choosing one is a move to its view. */
function PipelineSelect({ listing, current }: { listing: RequestState<PipelineListing>; current: string | undefined }) {
  const names = listing.status === 'loaded' ? listing.value.pipelines.map((p) => p.name) : [];
  const options = [...(current === undefined ? [{ value: '', label: 'Choose a pipeline', disabled: true }] : []), ...(current !== undefined && !names.includes(current) ? [current] : []).map((n) => ({ value: n, label: n })), ...names.map((n) => ({ value: n, label: n }))];
  return (
    <Select
      id="rg-pipeline-select"
      label="Pipeline"
      value={current ?? ''}
      options={options}
      onChange={(event) => {
        if (event.target.value !== '') navigate({ screen: 'pipeline', name: event.target.value });
      }}
    />
  );
}

export function PipelineScreen({ client, name, launch }: PipelineScreenProps) {
  const listing = usePipelines(client);
  if (name === undefined) return <Unchosen listing={listing} />;
  return <Reading client={client} name={name} listing={listing} launch={launch} />;
}

function Unchosen({ listing }: { listing: RequestState<PipelineListing> }) {
  const none = listing.status === 'loaded' && listing.value.pipelines.length === 0;
  return (
    <Sheet>
      <EmptyState
        heading="No pipeline chosen"
        action={
          none ? (
            <ButtonLink kind="primary" size="l" href={formatHash({ screen: 'editor' })}>
              Open the Editor
            </ButtonLink>
          ) : listing.status === 'loaded' ? (
            <PipelineSelect listing={listing} current={undefined} />
          ) : undefined
        }
      >
        {none ? 'This workspace holds no pipeline yet: write one in the Editor.' : 'Choose a pipeline to see each of its nodes against every benchmark it ran on.'}
      </EmptyState>
    </Sheet>
  );
}

function Reading({ client, name, listing, launch }: { client: ApiClient; name: string; listing: RequestState<PipelineListing>; launch: Launch | undefined }) {
  const [state, setState] = useState<RequestState<PipelineMatrix>>({ status: 'loading' });
  const latest = useRef(0);

  const read = useCallback(async () => {
    const mine = ++latest.current;
    setState({ status: 'loading' });
    const result = await client.get('/pipelines/{name}/matrix', { name }, { query: { include_available: true } });
    // Only the answer to the last matrix asked for lands.
    if (mine !== latest.current) return;
    if (result.ok) rememberPipeline(name);
    setState(result.ok ? { status: 'loaded', value: result.value } : { status: 'error', problem: result.problem });
  }, [client, name]);

  useEffect(() => {
    void read();
  }, [read]);

  if (state.status === 'loading') {
    return (
      <Sheet>
        <Loading label={`Reading the matrix of ${name}`} />
      </Sheet>
    );
  }
  if (state.status === 'error') return <ErrorState problem={state.problem} onRetry={() => void read()} />;
  return <Loaded matrix={state.value} listing={listing} launch={launch} />;
}

function Loaded({ matrix, listing, launch }: { matrix: PipelineMatrix; listing: RequestState<PipelineListing>; launch: Launch | undefined }) {
  const metrics = rankingMetrics(matrix);
  const [chosen, setChosen] = useState<string | null>(null);
  const metric = chosen !== null && metrics.includes(chosen) ? chosen : (metrics[0] ?? '');
  const empty = matrix.feeding_runs.length === 0 && matrix.columns.every((c) => c.run === null);
  const missing = missingCount(matrix);
  const unverified = matrix.columns.filter((c) => c.dataset_check !== null && c.dataset_check.status !== 'verified');

  const head = (
    <header className="rg-pipeline__head">
      <div className="rg-pipeline__bar">
        <PipelineSelect listing={listing} current={matrix.pipeline} />
        {metrics.length === 0 ? null : (
          <Select id="rg-pipeline-metric" label="Ranking metric" value={metric} options={metrics.map((m) => ({ value: m, label: m }))} onChange={(event) => setChosen(event.target.value)} />
        )}
        <ol className="rg-pipeline__shape" aria-label="Shape">
          {matrix.rows.map((row) => {
            const family = familyOfComponent(row.family);
            return (
              <li key={row.node} title={`${row.node}: ${family === null ? row.family : FAMILY_LABEL[family]}`}>
                {family === null ? <span className="rg-pipeline__note">{row.family}</span> : <FamilyTile family={family} labelled />}
              </li>
            );
          })}
        </ol>
      </div>
      {empty ? null : (
        <p className="rg-pipeline__subtitle">
          Derived from {runs(matrix.feeding_runs.length)}: each column is the most recent run on its benchmark; nothing here is stored. Canonical hash <code>{shortHash(matrix.pipeline_hash)}</code>
        </p>
      )}
    </header>
  );

  if (empty) {
    return (
      <Sheet>
        {head}
        <EmptyState
          heading={`No run of ${matrix.pipeline} yet`}
          action={
            <ButtonLink kind="primary" size="l" href={formatHash({ screen: 'editor', name: matrix.pipeline })}>
              Open in the Editor
            </ButtonLink>
          }
        >
          Each node of {matrix.pipeline} against every benchmark appears here once it has run. Launch it from the Editor.
        </EmptyState>
      </Sheet>
    );
  }

  const runMissing = () => {
    for (const column of matrix.missing) if (column.benchmark !== null) launch?.({ pipeline: matrix.pipeline, benchmark: column.benchmark });
  };

  return (
    <Sheet>
      {head}
      {matrix.unreadable.length === 0 && matrix.cache_errors.length === 0 ? null : (
        <div className="rg-pipeline__notes">
          {matrix.unreadable.length === 0 ? null : (
            <InlineMessage tone="warning" title={`${runs(matrix.unreadable.length)} could not be read, and count nowhere.`}>
              <ul>
                {matrix.unreadable.map((u) => (
                  <li key={u.id}>
                    <code>{shortHash(u.id)}</code>: <span>{u.reason}</span>
                  </li>
                ))}
              </ul>
            </InlineMessage>
          )}
          {matrix.cache_errors.length === 0 ? null : (
            <InlineMessage tone="info" title="Figures could not be cached; the matrix is complete.">
              <ul>
                {matrix.cache_errors.map((e) => (
                  <li key={e}>{e}</li>
                ))}
              </ul>
            </InlineMessage>
          )}
        </div>
      )}
      <Matrix matrix={matrix} metric={metric} launch={launch} />
      {unverified.length === 0 ? null : (
        <div className="rg-pipeline__notes">
          <ul>
            {unverified.map((c) => (
              <li key={c.dataset_version} className="rg-pipeline__note">
                {columnLabel(c)}: {c.dataset_check?.detail}.
              </li>
            ))}
          </ul>
        </div>
      )}
      <Section heading="Runs that feed it" caption="The most recent first. A run fills a column only when it is the most recent of the current pipeline, or of a prefix of it, on its benchmark.">
        <FeedingRuns runs={matrix.feeding_runs} pipeline={matrix.pipeline} />
      </Section>
      <div className="rg-pipeline__verdict">
        <p>{verdict(matrix)}</p>
        {missing === 0 ? null : launch === undefined ? (
          <Button kind="primary" disabled disabledReason={NO_LAUNCHER}>
            Run the {missing} missing {missing === 1 ? 'cell' : 'cells'}
          </Button>
        ) : (
          <Button kind="primary" onClick={runMissing}>
            Run the {missing} missing {missing === 1 ? 'cell' : 'cells'}
          </Button>
        )}
      </div>
    </Sheet>
  );
}
