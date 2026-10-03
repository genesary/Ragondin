// The Replay inspector: what the selected node did for this query — its meta
// line, its metric for the query and over the run, what it produced in rank
// order against its upstream, the context and the answer whole, and for the
// final node the verdict. Side by side, one column per run, the other run's
// node resolved by `counterpart`. ARCHITECTURE.md § The Replay screen.
import { FamilyTile, Inspector, InlineMessage, RunSwatch, familyOfComponent, type Family } from '../../design/index.ts';
import type { Graph, QueryTrace, RunQueries } from '../api/types.ts';
import { percent } from '../canvas/index.ts';
import { counterpart, formatMs, formatScore, listOf, terminalOf, verdict, type ListItem, type Reading } from './model.ts';

/** One run as the inspector reads it. */
export type Side = {
  letter: 'A' | 'B';
  /** The run's name: its pipeline's, or its short id. */
  name: string;
  graph: Graph;
  trace: QueryTrace;
  /** Its query listing, for the per-run metric and its ranking node; null while it is read. */
  queries: RunQueries | null;
};

const familyOf = (graph: Graph, id: string): Family => {
  if (graph.inputs.some((i) => i.id === id)) return 'query';
  return familyOfComponent(graph.nodes.find((n) => n.id === id)?.family ?? '') ?? 'control';
};
const implOf = (graph: Graph, id: string): string => {
  if (graph.inputs.some((i) => i.id === id)) return 'pipeline input';
  const node = graph.nodes.find((n) => n.id === id);
  return node === undefined ? '' : `${node.family}/${node.implementation}`;
};
const chunks = (n: number) => `${n} chunk${n === 1 ? '' : 's'}`;

/** One chunk of a node's list: rank, gold star and grade, move, and its text or its ids. */
export function ListItemView({ item, discarded = false }: { item: ListItem; discarded?: boolean }) {
  const gold = item.grade !== null && item.grade > 0;
  const body =
    item.text === null ? (
      <span className="rg-replay__ids">
        <code className="rg-replay__chunk">{item.chunk}</code>
        <span className="rg-replay__doc">{item.document}</span>
      </span>
    ) : (
      <span className="rg-replay__passage">
        <span className="rg-replay__ids">
          <code className="rg-replay__chunk">{item.chunk}</code>
          <span className="rg-replay__doc">{item.document}</span>
        </span>
        <span>{item.text}</span>
      </span>
    );
  return (
    <li
      className="rg-replay__item"
      data-gold={gold || undefined}
      data-move={discarded ? undefined : (item.move ?? undefined)}
      data-discarded={discarded || undefined}
      data-text={item.text === null ? 'none' : undefined}
    >
      <span className="rg-replay__rank">{discarded ? '' : item.rank}</span>
      <span className="rg-replay__star">
        {gold ? (
          <>
            <span aria-hidden="true">★</span>
            <span className="rg-visually-hidden">gold, grade {item.grade}</span>
          </>
        ) : null}
      </span>
      {discarded ? <del>{body}</del> : body}
      <span className="rg-replay__move">
        {discarded ? (
          `was rank ${item.former}`
        ) : item.move === 'up' || item.move === 'down' ? (
          <>
            <span aria-hidden="true">{item.move === 'up' ? '↑' : '↓'}</span>
            <span className="rg-visually-hidden">moved {item.move} from rank</span> {item.former}
          </>
        ) : item.move === 'new' ? (
          'new'
        ) : null}
      </span>
    </li>
  );
}

/** The per-run figure of the chosen metric at a node: its mean over the judged queries. */
function perRun(queries: RunQueries | null, node: string, metric: string | null): string | null {
  if (queries === null || metric === null) return null;
  const row = queries.nodes.find((n) => n.node === node);
  const value = row?.metrics?.[metric];
  if (row === undefined || value === undefined) return null;
  return `${formatScore(value)} over ${row.judged_queries} judged quer${row.judged_queries === 1 ? 'y' : 'ies'}`;
}

/** What one run's node did for this query. */
function NodeBody({ side, node, metric }: { side: Side; node: string; metric: string | null }) {
  const { graph, trace } = side;
  if (graph.inputs.some((i) => i.id === node)) {
    return (
      <div className="rg-replay__block">
        <h4>Query</h4>
        <p className="rg-replay__text">{trace.text ?? trace.query}</p>
      </div>
    );
  }
  const ran = trace.nodes.find((n) => n.node === node);
  if (ran === undefined) return <p className="rg-replay__meta">Not run: an earlier node failed on this query.</p>;
  const total = trace.nodes.reduce((sum, n) => sum + n.duration_nanos, 0);
  const share = total > 0 ? `, ${percent(ran.duration_nanos / total)} of this query's time` : '';
  if (ran.error !== null) {
    return (
      <InlineMessage tone="critical" title={`Failed after ${formatMs(ran.duration_nanos)} ms`}>
        {ran.error}
      </InlineMessage>
    );
  }
  const value = metric === null ? undefined : ran.metrics?.[metric];
  const run = perRun(side.queries, node, metric);
  const metricLine = [value === undefined ? null : `${formatScore(value)} on this query`, run].filter((p) => p !== null).join('; ');
  const list = listOf(graph, trace, node);
  const out = ran.output;
  return (
    <>
      <p className="rg-replay__meta">
        {formatMs(ran.duration_nanos)} ms{share}
      </p>
      {metricLine === '' || metric === null ? null : (
        <dl className="rg-replay__figure">
          <dt>{metric}</dt>
          <dd>{metricLine}</dd>
        </dl>
      )}
      {list === null ? null : (
        <div className="rg-replay__block">
          <h4>{out?.kind === 'context' ? 'Placed in the context' : 'Ranked'}</h4>
          <ol className="rg-replay__list" aria-label={`${out?.kind === 'context' ? 'Placed by' : 'Ranked by'} ${node}, ${chunks(list.kept.length)}`}>
            {list.kept.map((item) => (
              <ListItemView key={item.chunk} item={item} />
            ))}
          </ol>
          {list.discarded.length === 0 ? null : (
            <>
              <h4>Discarded ({list.discarded.length})</h4>
              <ol className="rg-replay__list" aria-label={`Discarded, ${chunks(list.discarded.length)}`}>
                {list.discarded.map((item) => (
                  <ListItemView key={item.chunk} item={item} discarded />
                ))}
              </ol>
            </>
          )}
        </div>
      )}
      {out?.kind === 'context' ? (
        <div className="rg-replay__block">
          <h4>Context</h4>
          <p className="rg-replay__text">{out.text}</p>
        </div>
      ) : null}
      {out?.kind === 'answer' ? (
        <div className="rg-replay__block">
          <h4>Answer</h4>
          <p className="rg-replay__text">{out.text}</p>
        </div>
      ) : null}
    </>
  );
}

/** One run's reading for the verdict: its score at its output, and its ranking node's gold ranks. */
function readingOf(side: Side, metric: string): Reading {
  const ranking = side.queries?.ranking_node ?? null;
  const gold = ranking === null ? null : (side.trace.nodes.find((n) => n.node === ranking)?.gold_ranks ?? null);
  return { score: side.trace.scores[metric], gold };
}

export type NodeInspectorProps = {
  /** The selected node. */
  node: string;
  /** The run it was selected in. */
  from: 'A' | 'B';
  /** One run, or two side by side, A first. */
  sides: readonly Side[];
  metric: string | null;
  /**
   * The run beside is chosen but not drawn yet: `sides` holds A alone, and
   * a verdict would read as A's one-run verdict, so it waits for B.
   */
  held?: boolean;
  onClose?: () => void;
};

/** The inspector for the selected node: one column, or two side by side with the counterpart resolved. */
export function NodeInspector({ node, from, sides, metric, held = false, onClose }: NodeInspectorProps) {
  const home = sides.find((s) => s.letter === from) ?? sides[0]!;
  const columns = sides.map((side) => {
    if (side === home) return { side, node, final: false };
    const other = counterpart(node, side);
    return { side, node: other?.node ?? null, final: other?.kind === 'final' };
  });
  // The verdict is the selected node's, when it is its own run's final node.
  const atEnd = node === terminalOf(home.graph);
  // The verdict reads each run's ranking node from its query listing: until
  // every listing has loaded it is left out, never said with a run's gold missing.
  const said =
    metric === null || !atEnd || held || sides.some((side) => side.queries === null)
      ? null
      : verdict(sides.length === 2 ? { metric, a: readingOf(sides[0]!, metric), b: readingOf(sides[1]!, metric) } : { metric, a: readingOf(home, metric) });
  const twoUp = sides.length === 2;
  return (
    <Inspector family={familyOf(home.graph, node)} title={node} impl={implOf(home.graph, node)} {...(onClose === undefined ? {} : { onClose })}>
      <div className="rg-replay__columns" data-columns={sides.length}>
        {columns.map(({ side, node: shown, final }) => {
          const content = (
            <>
              {final ? (
                <>
                  <p className="rg-replay__absent">No such node in {side.letter}.</p>
                  <h4 className="rg-replay__final">
                    <FamilyTile family={familyOf(side.graph, shown!)} />
                    {side.letter}&apos;s final output: {shown}
                  </h4>
                </>
              ) : null}
              {shown === null ? <p className="rg-replay__meta">Nothing ran in {side.letter} for this query.</p> : <NodeBody side={side} node={shown} metric={metric} />}
            </>
          );
          return twoUp ? (
            <section key={side.letter} className="rg-replay__column" aria-label={`${side.letter}, ${side.name}`}>
              <RunSwatch slot={side.letter === 'A' ? 'a' : 'b'} name={side.name} />
              {content}
            </section>
          ) : (
            <div key={side.letter} className="rg-replay__column">
              {content}
            </div>
          );
        })}
      </div>
      {said === null ? null : (
        <div className="rg-replay__block">
          <h4>Verdict</h4>
          <p className="rg-replay__verdict">{said}</p>
        </div>
      )}
    </Inspector>
  );
}
