// A pipeline's heading in the Runs table, set in design/'s Table as a group
// label: every name it goes by, each the way to its Pipeline screen, its
// shape as design/'s family tiles in pipeline order, and how many runs it
// holds. The shape comes with the listing, so the heading is whole when it is
// first drawn: nothing loads into it later, and nothing moves under it.
import { FAMILY_LABEL, FamilyTile } from '../../design/index.ts';
import { formatHash } from '../routes.ts';
import { shortHash, type RunGroup, type ShapeNode } from './model.ts';

export type GroupLabelProps = {
  group: RunGroup;
  /** The pipeline's shape, from the listing; null when the listing carries none for it. */
  shape: ShapeNode[] | null;
};

const runs = (n: number) => `${n.toLocaleString('en-US')} run${n === 1 ? '' : 's'}`;

export function GroupLabel({ group, shape }: GroupLabelProps) {
  return (
    <span className="rg-runs__group">
      {group.names.length === 0 ? (
        <a className="rg-runs__pipeline" href={formatHash({ screen: 'pipeline', name: group.key })}>
          <span className="rg-visually-hidden">pipeline </span>
          <code>{shortHash(group.pipeline)}</code>
        </a>
      ) : (
        <span className="rg-runs__names">
          {group.names.map((name) => (
            <a key={name} className="rg-runs__pipeline" href={formatHash({ screen: 'pipeline', name })}>
              {name}
            </a>
          ))}
        </span>
      )}
      {shape === null ? null : (
        <ol className="rg-runs__shape" aria-label="Shape">
          {shape.map((n) => (
            <li key={n.node} title={n.family === null ? `${n.node}: ${n.word}` : `${n.node}: ${FAMILY_LABEL[n.family]}`}>
              {n.family === null ? <span className="rg-runs__word">{n.word}</span> : <FamilyTile family={n.family} labelled />}
            </li>
          ))}
        </ol>
      )}
      <span className="rg-runs__count">{runs(group.rows.length)}</span>
    </span>
  );
}
