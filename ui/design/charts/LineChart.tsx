import { useState } from 'react';
import { ChartTooltip } from './ChartFrame.tsx';
import { linear, PLOT, ticks, type RunSeries } from './scale.ts';
import './Charts.css';

export type LineChartProps = {
  /** What the chart shows. */
  label: string;
  /** The positions along the axis, in order, e.g. the stages a ranking travels through. */
  x: readonly { id: string; label: string }[];
  /** The runs, in comparison order: one line each. */
  series: readonly RunSeries[];
  /** `values[series][x]`; null is a gap — the line breaks there and never crosses it. */
  values: readonly (readonly (number | null)[])[];
  /** Further marks at a position, beside the line: several nodes at one stage, each its own point. */
  dots?: readonly { series: number; x: number; value: number; label: string }[];
  /** The one scale every point is read on. */
  domain: readonly [number, number];
  format: (value: number) => string;
  /** What the hover layer says where a series has no value, e.g. "no stage here" — or, per point, the caller's reason. */
  gapLabel: string | ((series: number, x: number) => string);
};

/** The runs of consecutive present values, as index lists: what one unbroken segment joins. */
function runsOf(values: readonly (number | null)[]): number[][] {
  const out: number[][] = [];
  let current: number[] = [];
  values.forEach((v, i) => {
    if (v === null) {
      if (current.length > 0) out.push(current);
      current = [];
    } else current.push(i);
  });
  if (current.length > 0) out.push(current);
  return out;
}

/** The least distance, in SVG units, between two letters written at the same position. */
const LETTER_GAP = 12;

/**
 * Where each series' letter goes: beside its last point, and pushed apart
 * from the letters already placed at the same position, so two runs ending on
 * nearly the same value both stay legible. Null for a series with no value.
 */
function endLabels(values: readonly (readonly (number | null)[])[], y: (v: number) => number): ({ at: number; y: number } | null)[] {
  const raw = values.map((row) => {
    const at = row.reduce<number>((last, v, i) => (v === null ? last : i), -1);
    return at < 0 ? null : { at, y: y(row[at] as number) };
  });
  const order = raw.map((end, i) => ({ end, i })).filter((e) => e.end !== null) as { end: { at: number; y: number }; i: number }[];
  order.sort((a, b) => a.end.at - b.end.at || a.end.y - b.end.y);
  const out = [...raw];
  let previous: { at: number; y: number } | null = null;
  for (const { end, i } of order) {
    const placed: { at: number; y: number } = previous !== null && previous.at === end.at && end.y - previous.y < LETTER_GAP ? { at: end.at, y: previous.y + LETTER_GAP } : end;
    out[i] = placed;
    previous = placed;
  }
  return out;
}

/**
 * One line per run across ordered positions, on one scale. A missing value
 * breaks the line: a segment joins only neighbours both present, and a lone
 * value stands as a point. Each run's letter is written at its last point;
 * the baseline's line is dashed. Hovering a column shows every run's value,
 * or the gap's words.
 */
export function LineChart({ label, x, series, values, dots = [], domain, format, gapLabel }: LineChartProps) {
  const [hover, setHover] = useState<number | null>(null);
  const bottom = PLOT.height - PLOT.bottom;
  const y = linear(domain, [bottom, PLOT.top]);
  const step = (PLOT.width - PLOT.left - PLOT.right) / Math.max(x.length, 1);
  const cx = (i: number) => PLOT.left + step * (i + 0.5);
  const ends = endLabels(values, y);

  return (
    <>
      <svg viewBox={`0 0 ${PLOT.width} ${PLOT.height}`} aria-hidden="true" onPointerLeave={() => setHover(null)}>
        <g className="rg-chart__axis">
          {ticks(domain, 5).map((t) => (
            <g key={t}>
              <line className="rg-chart__grid" x1={PLOT.left} x2={PLOT.width - PLOT.right} y1={y(t)} y2={y(t)} />
              <text className="rg-chart__tick" x={PLOT.left - 6} y={y(t)} textAnchor="end" dominantBaseline="middle">
                {String(t)}
              </text>
            </g>
          ))}
          <line className="rg-chart__baseline" x1={PLOT.left} x2={PLOT.width - PLOT.right} y1={bottom} y2={bottom} />
        </g>
        {x.map((position, i) => (
          <text key={position.id} className="rg-chart__group" x={cx(i)} y={bottom + 20} textAnchor="middle">
            {position.label}
          </text>
        ))}
        {series.map((s, si) => {
          const row = values[si] ?? [];
          const end = ends[si];
          return (
            <g key={s.id}>
              {runsOf(row)
                .filter((run) => run.length > 1)
                .map((run) => (
                  <path key={run[0]} className="rg-chart__line" data-ink={s.ink} d={run.map((i, k) => `${k === 0 ? 'M' : 'L'}${cx(i)},${y(row[i] as number)}`).join('')} />
                ))}
              {row.map((v, i) => (v === null ? null : <circle key={i} className="rg-chart__point" data-ink={s.ink} cx={cx(i)} cy={y(v)} r={3.5} />))}
              {end === undefined || end === null ? null : (
                <text className="rg-chart__end" x={cx(end.at) + 8} y={end.y} dominantBaseline="middle">
                  {s.short}
                </text>
              )}
            </g>
          );
        })}
        {dots.map((d) => (
          <circle key={`${d.series}-${d.x}-${d.label}`} className="rg-chart__dot" data-ink={series[d.series]?.ink} cx={cx(d.x)} cy={y(d.value)} r={2.5} />
        ))}
        {x.map((position, i) => (
          <rect
            key={position.id}
            className="rg-chart__hit"
            x={PLOT.left + step * i}
            y={PLOT.top}
            width={step}
            height={bottom - PLOT.top}
            onPointerEnter={() => setHover(i)}
            onPointerLeave={() => setHover((h) => (h === i ? null : h))}
          />
        ))}
      </svg>
      {hover === null ? null : (
        <ChartTooltip x={cx(hover) / PLOT.width} y={PLOT.top / PLOT.height}>
          <strong>{x[hover]?.label ?? label}</strong>
          {series.map((s, si) => {
            const v = values[si]?.[hover] ?? null;
            return (
              <span key={s.id} className="rg-chart__tip-row">
                <span>{s.label}</span>
                <span>{v === null ? (typeof gapLabel === 'string' ? gapLabel : gapLabel(si, hover)) : format(v)}</span>
              </span>
            );
          })}
        </ChartTooltip>
      )}
    </>
  );
}
