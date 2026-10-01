// The run bar: the runs compared, each by its slot, pipeline and short id;
// the baseline selector; "+ add a run", which offers only runs of the same
// benchmark and lets the API refuse a sixth; and "Replay side by side".
import { useState } from 'react';
import { Button, RunSwatch, Select } from '../../design/index.ts';
import type { ApiProblem } from '../api/client.ts';
import type { Comparison, RunListing } from '../api/types.ts';
import { navigate } from '../routes.ts';
import { shortHash } from '../runs/model.ts';
import { ErrorState, type RequestState } from '../shell/states.tsx';
import { pipelineName, runSeries } from './model.ts';

export type RunBarProps = {
  comparison: Comparison;
  /** The runs as the address lists them. */
  ids: readonly string[];
  baseline: string;
  /** `GET /runs`, for the runs that can be added. */
  listing: RequestState<RunListing>;
  onRetryListing: () => void;
  /** Compares with one more run; the API's refusal, or null once the address names it. */
  onAdd: (id: string) => Promise<ApiProblem | null>;
};

/** Copies a run's full id; a browser that refuses the clipboard leaves the id in the hash's tooltip. */
const copy = (hash: string) => {
  void navigator.clipboard?.writeText(hash).catch(() => {});
};

export function RunBar({ comparison, ids, baseline, listing, onRetryListing, onAdd }: RunBarProps) {
  const series = runSeries(comparison);
  const [chosen, setChosen] = useState('');
  const [adding, setAdding] = useState(false);
  const [problem, setProblem] = useState<ApiProblem | null>(null);
  const dataset = comparison.ground_truth.expected.dataset_version;

  // Only runs of the same benchmark — the same dataset version — are offered:
  // a comparison across benchmarks is not one the API makes.
  const candidates = listing.status === 'loaded' ? listing.value.runs.filter((r) => r.dataset_version === dataset && !ids.includes(r.id)) : [];
  const placeholder =
    listing.status === 'loading' ? 'Reading runs…' : listing.status === 'error' ? 'Runs could not be read' : candidates.length === 0 ? 'No other run on this benchmark' : 'Choose a run…';

  const add = async () => {
    if (chosen === '' || adding) return;
    setAdding(true);
    const refused = await onAdd(chosen);
    setAdding(false);
    setProblem(refused);
    if (refused === null) setChosen('');
  };

  return (
    <>
      <ul className="rg-compare__runs" aria-label="Runs compared">
        {comparison.runs.map((run, i) => {
          const removable = i !== 0;
          const letter = series[i]?.short ?? '';
          return (
            <li key={run.id} className="rg-compare__run">
              <RunSwatch slot={series[i]?.ink ?? 'd'} name={pipelineName(run)} hash={run.id} onCopyHash={copy} />
              {!removable ? null : ids.length > 2 ? (
                <Button
                  size="s"
                  kind="quiet"
                  icon="close"
                  aria-label={`Remove run ${letter}`}
                  onClick={() => navigate({ screen: 'compare', ids: ids.filter((id) => id !== run.id), baseline })}
                >
                  {null}
                </Button>
              ) : (
                <Button size="s" kind="quiet" icon="close" aria-label={`Remove run ${letter}`} disabled disabledReason="A comparison needs a baseline and one other run.">
                  {null}
                </Button>
              )}
            </li>
          );
        })}
      </ul>
      <div className="rg-compare__controls">
        <Select
          id="compare-baseline"
          label="Baseline"
          value={baseline}
          options={comparison.runs.map((run) => ({ value: run.id, label: `${pipelineName(run)} · run ${shortHash(run.id)}` }))}
          // A new baseline is state within this view: written in place, as Runs writes its selection.
          onChange={(e) => navigate({ screen: 'compare', ids: [...ids], baseline: e.target.value }, { replace: true })}
        />
        <div className="rg-compare__add">
          <Select
            id="compare-add"
            label="Add a run"
            value={chosen}
            options={[{ value: '', label: placeholder }, ...candidates.map((r) => ({ value: r.id, label: `run ${shortHash(r.id)} · pipeline ${shortHash(r.pipeline)}` }))]}
            onChange={(e) => {
              setChosen(e.target.value);
              setProblem(null);
            }}
          />
          {chosen === '' ? (
            <Button icon="plus" disabled disabledReason="Choose a run of this benchmark to add.">
              Add
            </Button>
          ) : (
            <Button icon="plus" busy={adding} busyLabel="Adding" onClick={() => void add()}>
              Add
            </Button>
          )}
        </div>
        <Button icon="split" disabled disabledReason="Choose a query in the histogram: Replay opens one query beside the baseline.">
          Replay side by side
        </Button>
      </div>
      {listing.status === 'error' ? (
        <div className="rg-compare__problem">
          <ErrorState problem={listing.problem} onRetry={onRetryListing} />
        </div>
      ) : null}
      {problem === null ? null : (
        <div className="rg-compare__problem">
          <ErrorState problem={problem} />
        </div>
      )}
    </>
  );
}
