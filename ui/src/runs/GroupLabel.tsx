// A pipeline's heading in the Runs table, set in design/'s Table as a group
// label: its name as the way to the Pipeline screen, its shape as design/'s
// family tiles in pipeline order, and how many runs it holds. While its shape
// is read it says nothing itself: the screen holds one live region for every
// group's, so a list of pipelines is not announced once per heading.
import { Button, FAMILY_LABEL, FamilyTile } from '../../design/index.ts';
import { formatHash } from '../routes.ts';
import type { RequestState } from '../shell/states.tsx';
import { shortHash, type RunGroup, type ShapeNode } from './model.ts';

export type GroupLabelProps = {
  group: RunGroup;
  /** The pipeline's shape, read from one of its runs; null when the group has no run to read it from. */
  shape: RequestState<ShapeNode[]> | null;
  /** Reads the shape again after a failure. */
  onRetry: () => void;
};

const runs = (n: number) => `${n.toLocaleString('en-US')} run${n === 1 ? '' : 's'}`;

export function GroupLabel({ group, shape, onRetry }: GroupLabelProps) {
  return (
    <span className="rg-runs__group">
      <a className="rg-runs__pipeline" href={formatHash({ screen: 'pipeline', name: group.key })}>
        {group.name ?? (
          <>
            <span className="rg-visually-hidden">pipeline </span>
            <code>{shortHash(group.pipeline)}</code>
          </>
        )}
      </a>
      {shape?.status === 'error' ? (
        <span className="rg-runs__note">
          Shape not read: {shape.problem.message}{' '}
          <Button size="s" onClick={onRetry}>
            Retry
          </Button>
        </span>
      ) : null}
      {shape?.status === 'loaded' ? (
        <ol className="rg-runs__shape" aria-label="Shape">
          {shape.value.map((n) => (
            <li key={n.node} title={n.family === null ? `${n.node}: ${n.word}` : `${n.node}: ${FAMILY_LABEL[n.family]}`}>
              {n.family === null ? <span className="rg-runs__word">{n.word}</span> : <FamilyTile family={n.family} labelled />}
            </li>
          ))}
        </ol>
      ) : null}
      <span className="rg-runs__count">{runs(group.rows.length)}</span>
    </span>
  );
}
