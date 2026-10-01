// "Pair nodes…": the exceptional manual pairing (the front-end design, § 3).
// Two columns — the baseline's retrievers, fusion and reranker, and another
// run's — linked by drag or by two clicks; the automatic pairs the stages
// make drawn dashed, the pairs drawn by hand solid. Every change is the whole
// pairing posted through `POST /compare`, which keeps it; nothing is paired
// in the browser. ARCHITECTURE.md § The Compare screen.
import { useRef, useState, type DragEvent, type KeyboardEvent } from 'react';
import { Button, FamilyTile, InlineMessage, Select } from '../../design/index.ts';
import type { ApiProblem } from '../api/client.ts';
import type { Comparison, NodePair, Pairing } from '../api/types.ts';
import { ErrorState } from '../shell/states.tsx';
import { automaticLinks, manualPairs, pairableNodes, pipelineName, runSeries, stageLabel, type PairableNode } from './model.ts';

/** The height of one node row, in px: the 40px control token the rows are drawn at, so the links meet them. */
export const PAIR_ROW = 40;
const GUTTER = 64;

type Side = 'base' | 'other';
type Pick = { side: Side; node: string };

/**
 * What became of a posted pairing: kept, refused by the API, or overtaken by
 * a newer comparison asked for meanwhile — whose answer is the one shown, so
 * the panel says nothing of it.
 */
export type PairOutcome = { kind: 'kept' } | { kind: 'refused'; problem: ApiProblem } | { kind: 'superseded' };

export type PairingPanelProps = {
  id: string;
  comparison: Comparison;
  /** Posts the whole pairing between the baseline's pipeline and one other's. */
  onPair: (pairing: Pairing) => Promise<PairOutcome>;
};

const byHand = (n: number) => (n === 0 ? 'no pair by hand' : `${n} pair${n === 1 ? '' : 's'} by hand`);

export function PairingPanel({ id, comparison, onPair }: PairingPanelProps) {
  const series = runSeries(comparison);
  const [otherAt, setOtherAt] = useState(1);
  const [pick, setPick] = useState<Pick | null>(null);
  const [dragging, setDragging] = useState<Pick | null>(null);
  const [busy, setBusy] = useState(false);
  // Read and set in the same tick as a click: a second link made before the
  // first is answered must see it.
  const saving = useRef(false);
  const [problem, setProblem] = useState<ApiProblem | null>(null);
  const [message, setMessage] = useState('');

  const other = Math.min(otherAt, comparison.runs.length - 1);
  const base = comparison.runs[0];
  const them = comparison.runs[other];
  const baseName = series[0]?.label ?? 'baseline';
  const otherName = series[other]?.label ?? '';
  const letter = (side: Side) => (side === 'base' ? 'baseline' : (series[other]?.short ?? ''));
  const unnamed = [base, them].find((r) => r !== undefined && r.pipeline === null);

  /** Posts `pairs`, saying `doing` meanwhile and `done` once kept; refuses, in words, while another post is in flight. */
  const save = async (pairs: NodePair[], doing: string, done: string) => {
    if (base?.pipeline == null || them?.pipeline == null) return;
    setPick(null);
    if (saving.current) {
      setMessage('Still keeping the last pair: link again once it is kept.');
      return;
    }
    saving.current = true;
    setBusy(true);
    setProblem(null);
    setMessage(doing);
    const outcome = await onPair({ pipeline: base.pipeline, other: them.pipeline, pairs });
    saving.current = false;
    setBusy(false);
    setProblem(outcome.kind === 'refused' ? outcome.problem : null);
    setMessage(outcome.kind === 'kept' ? done : '');
  };

  const manual = manualPairs(comparison, other);
  const link = (a: Pick, b: Pick) => {
    const [node, mate] = a.side === 'base' ? [a.node, b.node] : [b.node, a.node];
    // A node is paired once: a new pair replaces any pair either node was in.
    const pairs = [...manual.filter((p) => p.node !== node && p.other !== mate), { node, other: mate }];
    void save(pairs, `Keeping ${node} paired with ${mate}…`, `${node} paired with ${mate}; ${byHand(pairs.length)}.`);
  };

  const choose = (target: Pick) => {
    if (pick === null || pick.side === target.side) {
      setMessage('');
      setPick(pick?.side === target.side && pick.node === target.node ? null : target);
      return;
    }
    link(pick, target);
  };

  const left = pairableNodes(comparison, 0);
  const right = pairableNodes(comparison, other);
  const rowOf = (nodes: PairableNode[], name: string) => nodes.findIndex((n) => n.node === name);
  const y = (row: number) => row * PAIR_ROW + PAIR_ROW / 2;
  const links = [
    ...automaticLinks(comparison, other).map((p) => ({ ...p, source: 'automatic' as const })),
    ...manual.map((p) => ({ ...p, source: 'manual' as const })),
  ].filter((p) => rowOf(left, p.node) >= 0 && rowOf(right, p.other) >= 0);

  const column = (side: Side, nodes: PairableNode[], name: string) => (
    <ul className="rg-pair__column" aria-label={name}>
      {nodes.map((n) => {
        const me = { side, node: n.node };
        return (
          <li key={n.node}>
            <button
              type="button"
              className="rg-pair__node"
              aria-label={`${n.node}, ${letter(side)}`}
              aria-pressed={pick?.side === side && pick.node === n.node}
              draggable
              onClick={() => choose(me)}
              onDragStart={(e: DragEvent) => {
                e.dataTransfer?.setData('text/plain', n.node);
                setDragging(me);
              }}
              onDragEnd={() => setDragging(null)}
              onDragOver={(e: DragEvent) => {
                if (dragging !== null && dragging.side !== side) e.preventDefault();
              }}
              onDrop={(e: DragEvent) => {
                e.preventDefault();
                if (dragging !== null && dragging.side !== side) link(dragging, me);
                setDragging(null);
              }}
            >
              {n.family === null ? null : <FamilyTile family={n.family} />}
              <span className="rg-pair__name">{n.node}</span>
              <span className="rg-pair__stage">{stageLabel(n.stage)}</span>
            </button>
          </li>
        );
      })}
    </ul>
  );

  const onKeyDown = (e: KeyboardEvent) => {
    if (e.key === 'Escape' && pick !== null) {
      e.stopPropagation();
      setPick(null);
    }
  };

  return (
    <section id={id} className="rg-pair" aria-label="Pair nodes" aria-busy={busy || undefined} onKeyDown={onKeyDown}>
      <p className="rg-compare__note">
        A pair moves the other run&apos;s node into the stage of the baseline&apos;s, and is kept for these two pipelines. Click a node, then its pair in the other column, or drag one onto the other.
      </p>
      <div className="rg-pair__choose">
        <Select
          id={`${id}-other`}
          label="Pair the baseline with"
          value={them?.id ?? ''}
          options={comparison.runs.slice(1).map((r, i) => ({ value: r.id, label: series[i + 1]?.label ?? r.id }))}
          onChange={(e) => {
            setOtherAt(Math.max(1, comparison.runs.findIndex((r) => r.id === e.target.value)));
            setPick(null);
            setProblem(null);
            setMessage('');
          }}
        />
      </div>
      {unnamed !== undefined ? (
        <InlineMessage tone="info" title="Paired automatically only">
          Run {pipelineName(unnamed)} matches no pipeline document of the workspace, or several: it pairs automatically only. A pairing is kept between two pipeline documents.
        </InlineMessage>
      ) : (
        <>
          <div className="rg-pair__grid">
            <span className="rg-pair__head">{baseName}</span>
            <span />
            <span className="rg-pair__head">{otherName}</span>
            {column('base', left, baseName)}
            <svg className="rg-pair__links" width={GUTTER} height={Math.max(left.length, right.length, 1) * PAIR_ROW} aria-hidden="true">
              {links.map((l) => (
                <line key={`${l.source}-${l.node}-${l.other}`} className="rg-pair__link" data-source={l.source} x1={0} x2={GUTTER} y1={y(rowOf(left, l.node))} y2={y(rowOf(right, l.other))} />
              ))}
            </svg>
            {column('other', right, otherName)}
          </div>
          <p className="rg-pair__status" role="status">
            {pick === null ? message : `${pick.node} picked: choose a node of ${pick.side === 'base' ? otherName : 'the baseline'} to pair it with.`}
          </p>
          <div className="rg-pair__legend" aria-hidden="true">
            <span data-source="automatic">automatic, by stage</span>
            <span data-source="manual">drawn by hand</span>
          </div>
          {/* Always rendered, its height reserved, so the first pair drawn moves nothing below it. */}
          <ul className="rg-pair__pairs" aria-label="Pairs drawn by hand">
            {manual.length === 0 ? <li className="rg-pair__none">None yet.</li> : null}
            {manual.map((p) => {
              const rest = manual.filter((q) => q !== p);
              return (
                <li key={`${p.node}-${p.other}`}>
                  <span>
                    {p.node} ↔ {p.other}
                    {p.label == null ? null : ` — ${p.label}`}
                  </span>
                  <Button
                    size="s"
                    kind="quiet"
                    icon="close"
                    aria-label={`Remove the pair ${p.node} and ${p.other}`}
                    onClick={() => void save(rest, `Removing the pair ${p.node} and ${p.other}…`, `${p.node} and ${p.other} unpaired; ${byHand(rest.length)}.`)}
                  >
                    Remove
                  </Button>
                </li>
              );
            })}
          </ul>
          <div className="rg-pair__actions">
            {manual.length === 0 ? (
              <Button size="s" disabled disabledReason="No pair is drawn by hand between these two pipelines.">
                Reset to automatic
              </Button>
            ) : (
              <Button size="s" onClick={() => void save([], 'Resetting to automatic…', 'Reset to automatic.')}>
                Reset to automatic
              </Button>
            )}
          </div>
        </>
      )}
      {problem === null ? null : <ErrorState problem={problem} />}
    </section>
  );
}
