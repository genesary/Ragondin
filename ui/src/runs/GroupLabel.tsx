// A pipeline's heading in the Runs table, set in design/'s Table as a group
// label: the name or names it is grouped under, each the way to its Pipeline
// screen — or its short hash, when no name reaches it — its
// shape as design/'s family tiles in pipeline order, and how many runs it
// holds. A name the workspace no longer holds, or that another stored name
// differs from only in case, is said so, unlinked: its address would land on a
// pipeline that is not found, or be refused as a case alias. The shape and
// whether each name is held come with the listing, so the heading is whole
// when it is first drawn: nothing loads into it later, and nothing moves under
// it.
import { Fragment } from 'react';
import { FAMILY_LABEL, FamilyTile } from '../../design/index.ts';
import { formatHash } from '../routes.ts';
import { shortHash, type RunGroup, type ShapeNode } from './model.ts';

export type GroupLabelProps = {
  group: RunGroup;
  /** The pipeline's shape, from the listing; null when the listing carries none for it. */
  shape: ShapeNode[] | null;
};

/** Why a recorded name is not a link, in words, by what the listing says of it. */
// `other_case` is true whether or not the name is also stored as given: the API refuses it either way.
const NOT_HELD = { gone: 'no longer a document in this workspace', other_case: 'refused: another spelling differs only in case', unchecked: 'not checked against the workspace' } as const;

const runs = (n: number) => `${n.toLocaleString('en-US')} run${n === 1 ? '' : 's'}`;

export function GroupLabel({ group, shape }: GroupLabelProps) {
  return (
    <span className="rg-runs__group">
      {group.names.length === 0 ? (
        <a className="rg-runs__pipeline" href={formatHash({ screen: 'pipeline', name: group.pipeline })}>
          <span className="rg-visually-hidden">pipeline </span>
          <code>{shortHash(group.pipeline)}</code>
        </a>
      ) : (
        <span className="rg-runs__names">
          {group.names.map((name, i) => {
            // A model that gave no word for a name fails closed, unlinked.
            const held = group.held[i] ?? 'unchecked';
            return (
              <Fragment key={name}>
                {/* Heard, not seen: the gap shows the names apart, and this keeps them apart for a screen reader. */}
                {i === 0 ? null : <span className="rg-visually-hidden">, </span>}
                {held === 'exactly' ? (
                  <a className="rg-runs__pipeline" href={formatHash({ screen: 'pipeline', name })}>
                    {name}
                  </a>
                ) : (
                  // A hash match is a current document, so it is other than held only when the API refuses it.
                  <span className="rg-runs__gone">
                    {name} <span className="rg-runs__gone-why">{NOT_HELD[held]}</span>
                  </span>
                )}
              </Fragment>
            );
          })}
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
