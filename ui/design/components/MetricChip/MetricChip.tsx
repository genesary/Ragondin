import type { ReactNode } from 'react';
import { Glyph } from '../../glyphs/Glyph.tsx';
import './MetricChip.css';

export type DeltaProps = {
  /** What the change means against the baseline; the colour follows this. */
  meaning: 'better' | 'worse' | 'same';
  /** Which way the number moved; the arrow follows this. Higher latency is up and worse. */
  direction: 'up' | 'down' | 'none';
  /** The signed value, e.g. "+0.073" or "−0.3". */
  children: ReactNode;
};

const SPOKEN: Record<DeltaProps['meaning'], string> = { better: 'better', worse: 'worse', same: 'unchanged' };

/** A change against the baseline: sign, arrow and word, never colour alone. */
export function Delta({ meaning, direction, children }: DeltaProps) {
  return (
    <span className="rg-delta" data-meaning={meaning} data-direction={direction}>
      {direction === 'none' ? null : <Glyph name={direction} />}
      {children}
      <span className="rg-visually-hidden"> {SPOKEN[meaning]}</span>
    </span>
  );
}

export type MetricChipProps = {
  /** e.g. "nDCG@10". */
  name: string;
  /** Formatted by the caller: four decimals for ranking metrics, one for EM/F1, whole ms for latency. */
  value: string;
  /** The single best value in the view, once. */
  best?: boolean;
  delta?: ReactNode;
};

export function MetricChip({ name, value, best = false, delta }: MetricChipProps) {
  return (
    <span className={best ? 'rg-metric is-best' : 'rg-metric'}>
      <span className="rg-metric__k">{name}</span>
      <span className="rg-metric__v">{value}</span>
      {best ? <span className="rg-visually-hidden">best</span> : null}
      {delta}
    </span>
  );
}
