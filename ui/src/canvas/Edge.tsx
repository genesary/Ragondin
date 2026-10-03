import type { PortKind } from './model.ts';

/** The design system's edge: a horizontal cubic between two port centres. */
export function edgePath(x1: number, y1: number, x2: number, y2: number): string {
  const dx = Math.max(40, Math.abs(x2 - x1) * 0.5);
  return `M${x1},${y1} C${x1 + dx},${y1} ${x2 - dx},${y2} ${x2},${y2}`;
}

export type EdgeLineProps = {
  x1: number;
  y1: number;
  x2: number;
  y2: number;
  from: string;
  to: string;
  port: number;
  kind: PortKind;
  /** The validation named this edge: drawn dashed, so it reads without colour. */
  invalid?: boolean;
};

/** One edge, at rest: which ends it joins and what it carries are on the element, for a test or a stylesheet to read. */
export function EdgeLine({ x1, y1, x2, y2, from, to, port, kind, invalid = false }: EdgeLineProps) {
  return <path className="rg-edge" d={edgePath(x1, y1, x2, y2)} data-from={from} data-to={to} data-port={port} data-kind={kind} data-invalid={invalid || undefined} />;
}
