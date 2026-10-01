import type { PortKind } from './model.ts';
import './Port.css';

export type PortSide = 'in' | 'out';

/** A port as the card places it: which side, which kind, its index on that side and its top offset. */
export type PortProps = { side: PortSide; kind: PortKind; index: number; top: number };

/** What a person calls each kind, the words the legend and a port's tooltip use. */
export const PORT_LABEL: Record<PortKind, string> = {
  query: 'query',
  chunks: 'candidates',
  context: 'context',
  answer: 'answer',
  opaque: 'other',
};

/** The kinds in the order the legend lists them. */
export const PORT_KINDS: readonly PortKind[] = ['query', 'chunks', 'context', 'answer', 'opaque'];

// The first port sits level with the card's head; each next one a step lower.
const FIRST_PORT_TOP = 17;
const PORT_STEP = 20;
export const portTop = (index: number) => FIRST_PORT_TOP + index * PORT_STEP;

export const portTitle = (side: PortSide, kind: PortKind) => `${side === 'in' ? 'input' : 'output'}: ${PORT_LABEL[kind]}`;

/**
 * A port's mark: its shape says its kind — a circle for the query, a square
 * for candidates, a diamond for context, a leaf for the answer, a dashed ring
 * for anything else — so a connection reads without colour.
 */
export function PortMark({ side, kind, top }: PortProps) {
  return <span className="rg-port" data-side={side} data-kind={kind} style={{ top }} title={portTitle(side, kind)} />;
}

/** The mark alone, in flow, for the legend. */
export function PortSwatch({ kind }: { kind: PortKind }) {
  return <span className="rg-port" data-kind={kind} data-swatch="true" aria-hidden="true" />;
}
