import type { PortKind } from './model.ts';
import './Port.css';

export type PortSide = 'in' | 'out';

/** A port as the card places it: which side, which kind, its index on that side, its top offset, and whether an edge meets it. */
export type PortProps = { side: PortSide; kind: PortKind; index: number; top: number; connected: boolean };

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
 * for candidates, a diamond for context, a leaf for the answer, and for
 * anything else a ring around a dot that stays hollow when connected and in
 * replay — so a connection reads without colour. A connected port is filled.
 */
export function PortMark({ side, kind, top, connected }: PortProps) {
  return (
    <span className="rg-port" data-side={side} data-kind={kind} data-connected={connected || undefined} style={{ top }} title={portTitle(side, kind)}>
      <PortDot kind={kind} />
    </span>
  );
}

/** The dot inside the `opaque` ring; no other kind has one. */
export function PortDot({ kind }: { kind: PortKind }) {
  return kind === 'opaque' ? <i className="rg-port__dot" /> : null;
}

/** The mark alone, in flow, for the legend. */
export function PortSwatch({ kind }: { kind: PortKind }) {
  return (
    <span className="rg-port" data-kind={kind} data-swatch="true" aria-hidden="true">
      <PortDot kind={kind} />
    </span>
  );
}
