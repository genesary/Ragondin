// A pipeline's heading in the Runs table: its name as the way to the Pipeline
// screen, its shape as design/'s family tiles in pipeline order, and how many
// runs it holds. A row-group header, so assistive technology reads each run
// under its pipeline.
import { FAMILY_LABEL, FamilyTile } from '../../design/index.ts';
import { formatHash } from '../routes.ts';
import { Loading, type RequestState } from '../shell/states.tsx';
import { shortHash, type RunGroup, type ShapeNode } from './model.ts';

export type GroupHeaderProps = {
  group: RunGroup;
  /** The pipeline's shape, read from one of its runs. */
  shape: RequestState<ShapeNode[]>;
  /** How many columns the header spans. */
  columns: number;
};

const runs = (n: number) => `${n.toLocaleString('en-US')} run${n === 1 ? '' : 's'}`;

export function GroupHeader({ group, shape, columns }: GroupHeaderProps) {
  return (
    <tr className="rg-table__group">
      <th scope="rowgroup" colSpan={columns}>
        <span className="rg-runs__group">
          <a className="rg-runs__pipeline" href={formatHash({ screen: 'pipeline', name: group.key })}>
            {group.name ?? (
              <>
                <span className="rg-visually-hidden">pipeline </span>
                <code>{shortHash(group.pipeline)}</code>
              </>
            )}
          </a>
          {shape.status === 'loading' ? <Loading label="Reading the shape" /> : null}
          {shape.status === 'error' ? <span className="rg-runs__note">Shape not read: {shape.problem.message}</span> : null}
          {shape.status === 'loaded' ? (
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
      </th>
    </tr>
  );
}
