// The runs that feed the matrix, the most recent first, as the API lists
// them: each the way to its replay, a prefix run labelled, a run whose
// content changed since its launch said so with its parameter difference,
// and ADR-C39 § 4's two facts — what it was launched as, and which current
// documents its content is — side by side, never resolved into one name.
// ARCHITECTURE.md § The Pipeline screen.
import { PrefixLabel, Table, type TableRow } from '../../design/index.ts';
import type { ConfigurationMatrix, FeedingRun } from '../api/types.ts';
import { formatParameter, parameterName } from '../compare/model.ts';
import { formatHash } from '../routes.ts';
import { shortHash } from '../runs/model.ts';
import { contentFact, launchFact, sinceChangedLabel } from './model.ts';

const started = (ms: number) => new Date(ms).toLocaleString('en-US', { dateStyle: 'medium', timeStyle: 'short' });

/** A since-changed run's parameter difference: the current document's value first, then the run's. */
function Difference({ difference, pipeline, run }: { difference: ConfigurationMatrix; pipeline: string; run: string }) {
  if (difference.kind === 'unavailable') return <p className="rg-pipeline__note">The difference cannot be read: {difference.reason}</p>;
  if (difference.parameters.length === 0) {
    return <p className="rg-pipeline__note">{difference.same_logical_form ? 'No parameter differs.' : 'No parameter differs: only how the nodes are wired.'}</p>;
  }
  const rows: TableRow[] = difference.parameters.map((row) => ({
    id: `${row.node}/${parameterName(row.key)}`,
    cells: [row.node, parameterName(row.key), formatParameter(row.values[0] ?? null), formatParameter(row.values[1] ?? null)],
  }));
  return (
    <Table
      caption={`What differs between ${pipeline} now and run ${shortHash(run)}`}
      rowHeaders
      region
      columns={[
        { id: 'node', label: 'Node' },
        { id: 'key', label: 'Parameter' },
        { id: 'now', label: `${pipeline} now` },
        { id: 'run', label: 'This run' },
      ]}
      rows={rows}
    />
  );
}

export function FeedingRuns({ runs, pipeline }: { runs: readonly FeedingRun[]; pipeline: string }) {
  return (
    <ul className="rg-pipeline__runs" aria-label="Runs that feed the matrix">
      {runs.map((run) => {
        const changed = sinceChangedLabel(run, pipeline);
        const benchmark = run.benchmark_names.length === 0 ? `dataset ${shortHash(run.dataset_version)}` : run.benchmark_names.join(', ');
        return (
          <li key={run.run} className="rg-pipeline__run">
            <span className="rg-pipeline__runhead">
              <a href={formatHash({ screen: 'replay', run: run.run })}>
                run <code>{shortHash(run.run)}</code>
              </a>
              <span>on {benchmark}</span>
              {run.started_at_ms === null ? null : <time dateTime={new Date(run.started_at_ms).toISOString()}>{started(run.started_at_ms)}</time>}
              {run.prefix_of === null ? null : (
                <span className="rg-pipeline__label">
                  <PrefixLabel parent={run.prefix_of.pipeline} upTo={run.prefix_of.up_to} />
                </span>
              )}
              {changed === null ? null : <span className="rg-pipeline__label">{changed}</span>}
              <span className="rg-pipeline__label">{run.fills_column ? 'fills its column' : 'fills no cell'}</span>
            </span>
            <p className="rg-pipeline__facts">
              <span>{launchFact(run)}</span>
              {/* Heard, not seen: the gap shows the two facts apart, and this keeps them apart for a screen reader. */}
              <span className="rg-visually-hidden">. </span>
              <span>{contentFact(run)}</span>
            </p>
            {run.content_since_changed === null ? null : <Difference difference={run.content_since_changed.difference} pipeline={pipeline} run={run.run} />}
          </li>
        );
      })}
    </ul>
  );
}
