import { useState } from 'react';
import type { Family } from '../glyphs/Glyph.tsx';
import { ChartTooltip } from './ChartFrame.tsx';
import { linear, PLOT, ticks } from './scale.ts';
import './Charts.css';

export type StackSegment = {
  id: string;
  label: string;
  value: number;
  /** The node family whose pigment fills it; null for one with no pigment of its own, drawn neutral. */
  family: Family | null;
};

export type StackedBarChartProps = {
  /** What the chart shows. */
  label: string;
  /** One horizontal bar each, top to bottom. */
  bars: readonly { id: string; label: string }[];
  /** `segments[bar]`, in the order they stack from the axis. */
  segments: readonly (readonly StackSegment[])[];
  format: (value: number) => string;
};

/** A domain from zero to the first round tick at or past `max`. */
function domainTo(max: number): [number, number] {
  if (max <= 0) return [0, 1];
  const t = ticks([0, max], 4);
  const step = (t[1] ?? max) - (t[0] ?? 0);
  const last = t[t.length - 1] ?? max;
  return [0, last >= max ? last : last + step];
}

/**
 * Horizontal stacked bars on one scale: per bar, its segments end to end,
 * each filled from its family's pigment — this chart's palette, never the run
 * inks — and its total written at its end. Hovering a bar's row lists its
 * segments.
 */
export function StackedBarChart({ label, bars, segments, format }: StackedBarChartProps) {
  const [hover, setHover] = useState<number | null>(null);
  const bottom = PLOT.height - PLOT.bottom;
  const totals = bars.map((_, b) => (segments[b] ?? []).reduce((sum, s) => sum + s.value, 0));
  const domain = domainTo(Math.max(0, ...totals));
  const x = linear(domain, [PLOT.left, PLOT.width - PLOT.right]);
  const band = (bottom - PLOT.top) / Math.max(bars.length, 1);
  const thickness = Math.min(band * 0.5, 24);
  const barY = (b: number) => PLOT.top + band * b + (band - thickness) / 2 + 6;

  return (
    <>
      <svg viewBox={`0 0 ${PLOT.width} ${PLOT.height}`} aria-hidden="true" onPointerLeave={() => setHover(null)}>
        <g className="rg-chart__axis">
          {ticks(domain, 4).map((t) => (
            <g key={t}>
              <line className="rg-chart__grid" x1={x(t)} x2={x(t)} y1={PLOT.top} y2={bottom} />
              <text className="rg-chart__tick" x={x(t)} y={bottom + 14} textAnchor="middle">
                {format(t)}
              </text>
            </g>
          ))}
          <line className="rg-chart__baseline" x1={PLOT.left} x2={PLOT.left} y1={PLOT.top} y2={bottom} />
        </g>
        {bars.map((bar, b) => {
          let at = 0;
          return (
            <g key={bar.id}>
              <text className="rg-chart__group" x={PLOT.left} y={barY(b) - 5}>
                {bar.label}
              </text>
              <g className="rg-chart__stack">
                {(segments[b] ?? []).map((s) => {
                  const from = at;
                  at += s.value;
                  return <rect key={s.id} className="rg-chart__seg" data-family={s.family ?? 'none'} x={x(from)} y={barY(b)} width={x(at) - x(from)} height={thickness} />;
                })}
              </g>
              <text className="rg-chart__total" x={x(totals[b] ?? 0) + 6} y={barY(b) + thickness / 2} dominantBaseline="middle">
                {format(totals[b] ?? 0)}
              </text>
              <rect
                className="rg-chart__hit"
                x={PLOT.left}
                y={PLOT.top + band * b}
                width={PLOT.width - PLOT.left - PLOT.right}
                height={band}
                onPointerEnter={() => setHover(b)}
                onPointerLeave={() => setHover((h) => (h === b ? null : h))}
              />
            </g>
          );
        })}
      </svg>
      {hover === null ? null : (
        <ChartTooltip x={0.5} y={barY(hover) / PLOT.height}>
          <strong>{bars[hover]?.label ?? label}</strong>
          {(segments[hover] ?? []).map((s) => (
            <span key={s.id} className="rg-chart__tip-row">
              <span>{s.label}</span>
              <span>{format(s.value)}</span>
            </span>
          ))}
          <span className="rg-chart__tip-row">
            <span>total</span>
            <span>{format(totals[hover] ?? 0)}</span>
          </span>
        </ChartTooltip>
      )}
    </>
  );
}
