import { useState } from 'react';
import { ChartTooltip } from './ChartFrame.tsx';
import { linear, PLOT, ticks, type RunSeries } from './scale.ts';
import './Charts.css';

export type BarChartProps = {
  /** What the chart shows; the tooltip's heading. */
  label: string;
  /** The groups along the axis, e.g. one per metric. */
  groups: readonly { id: string; label: string }[];
  /** The runs, in comparison order: one bar each per group. */
  series: readonly RunSeries[];
  /** `values[group][series]`; null draws no bar — a value not recorded is never a zero. */
  values: readonly (readonly (number | null)[])[];
  /** The one scale every bar is read on. */
  domain: readonly [number, number];
  format: (value: number) => string;
  /** Whether a bar holds its group's best value: labelled with a star and its number. */
  best?: (group: number, series: number) => boolean;
};

/**
 * Grouped bars on one scale: one bar per run in each group, each run's ink
 * with its letter written under it, the baseline a dashed neutral outline,
 * the best value of a group labelled. Hovering a group shows every run's
 * value. Drawn for the eye; the caller's table is the text equivalent.
 */
export function BarChart({ label, groups, series, values, domain, format, best }: BarChartProps) {
  const [hover, setHover] = useState<number | null>(null);
  const bottom = PLOT.height - PLOT.bottom;
  const y = linear(domain, [bottom, PLOT.top]);
  const band = (PLOT.width - PLOT.left - PLOT.right) / Math.max(groups.length, 1);
  const barWidth = (band * 0.72) / Math.max(series.length, 1);
  const groupX = (g: number) => PLOT.left + g * band;
  const barX = (g: number, s: number) => groupX(g) + band * 0.14 + s * barWidth;

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
        {groups.map((group, g) => (
          <g key={group.id}>
            {series.map((s, i) => {
              const value = values[g]?.[i] ?? null;
              const x = barX(g, i);
              return (
                <g key={s.id}>
                  {value === null ? null : (
                    <rect className="rg-chart__bar" data-ink={s.ink} x={x + 1} y={y(value)} width={Math.max(barWidth - 2, 1)} height={bottom - y(value)} />
                  )}
                  {value !== null && best?.(g, i) ? (
                    <text className="rg-chart__best" x={x + barWidth / 2} y={y(value) - 6} textAnchor="middle">
                      {`★ ${format(value)}`}
                    </text>
                  ) : null}
                  <text className="rg-chart__letter" x={x + barWidth / 2} y={bottom + 14} textAnchor="middle">
                    {s.short}
                  </text>
                </g>
              );
            })}
            <text className="rg-chart__group" x={groupX(g) + band / 2} y={bottom + 34} textAnchor="middle">
              {group.label}
            </text>
            <rect
              className="rg-chart__hit"
              x={groupX(g)}
              y={PLOT.top}
              width={band}
              height={bottom - PLOT.top}
              onPointerEnter={() => setHover(g)}
              onPointerLeave={() => setHover((h) => (h === g ? null : h))}
            />
          </g>
        ))}
      </svg>
      {hover === null ? null : (
        <ChartTooltip x={(groupX(hover) + band / 2) / PLOT.width} y={PLOT.top / PLOT.height}>
          <strong>{groups[hover]?.label ?? label}</strong>
          {series.map((s, i) => {
            const value = values[hover]?.[i] ?? null;
            return (
              <span key={s.id} className="rg-chart__tip-row">
                <span>{s.label}</span>
                <span>{value === null ? 'not recorded' : format(value)}</span>
              </span>
            );
          })}
        </ChartTooltip>
      )}
    </>
  );
}
