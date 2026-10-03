// A pipeline's heading in the Runs table, set in design/'s Table as a group
// label: the name or names it is grouped under, each the way to its Pipeline
// screen — or its short hash, when no name reaches it — its
// shape as design/'s family tiles in pipeline order, and how many runs it
// holds. A recorded name the workspace no longer holds is said so, unlinked:
// its address would land on a pipeline that is not found. The shape and the
// workspace's documents come with the listing, so the heading is whole when it
// is first drawn: nothing loads into it later, and nothing moves under it.
import { Fragment } from 'react';
import { FAMILY_LABEL, FamilyTile } from '../../design/index.ts';
import { formatHash } from '../routes.ts';
import { shortHash, type RunGroup, type ShapeNode } from './model.ts';

export type GroupLabelProps = {
  group: RunGroup;
  /** The pipeline's shape, from the listing; null when the listing carries none for it. */
  shape: ShapeNode[] | null;
  /** The workspace's pipeline documents, by name; null when they could not be listed, and then every name links. */
  documents: ReadonlySet<string> | null;
};

const runs = (n: number) => `${n.toLocaleString('en-US')} run${n === 1 ? '' : 's'}`;

export function GroupLabel({ group, shape, documents }: GroupLabelProps) {
  return (
    <span className="rg-runs__group">
      {group.names.length === 0 ? (
        <a className="rg-runs__pipeline" href={formatHash({ screen: 'pipeline', name: group.pipeline })}>
          <span className="rg-visually-hidden">pipeline </span>
          <code>{shortHash(group.pipeline)}</code>
        </a>
      ) : (
        <span className="rg-runs__names">
          {group.names.map((name, i) => (
            <Fragment key={name}>
              {/* Heard, not seen: the gap shows the names apart, and this keeps them apart for a screen reader. */}
              {i === 0 ? null : <span className="rg-visually-hidden">, </span>}
              {documents === null || documents.has(name) ? (
                <a className="rg-runs__pipeline" href={formatHash({ screen: 'pipeline', name })}>
                  {name}
                </a>
              ) : (
                // Only a recorded name can be gone: a hash match is a current document by definition.
                <span className="rg-runs__gone">
                  {name} <span className="rg-runs__gone-why">no longer a document in this workspace</span>
                </span>
              )}
            </Fragment>
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
