import { Fragment, useState } from 'react';
import { Glyph, type Family } from '../glyphs/Glyph.tsx';
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

/**
 * The label's type size in SVG units: the 11px of `--type-micro`, the type
 * Charts.css draws a segment's name in. A change to either is a change to
 * both; the chart's test holds the token to 11px.
 */
const LABEL_SIZE = 11;
/**
 * An upper bound on the width of `name` in that type, in SVG units: per
 * character, at least the advance the shipped Wix Madefor Text Medium gives
 * it — m, w, their capitals, @ and % 1.06 em, another capital 0.86 em,
 * anything else 0.66 em — so a name judged to fit does, and is never clipped
 * into what would read as another id. It is an estimate rather than a
 * measurement because a measurement needs the text laid out first, and
 * labelling after that would move the plot under the reader once the font
 * arrives.
 */
function nameWidth(name: string): number {
  let ems = 0;
  for (const c of name) ems += /[mwMW@%]/.test(c) ? 1.06 : /[A-Z]/.test(c) ? 0.86 : 0.66;
  return ems * LABEL_SIZE;
}
/** Room left between a label and its segment's edges, in SVG units. */
const INSET = 4;
/**
 * The narrowest a segment that took any time is drawn, in SVG units: a
 * fusion of microseconds beside a reranker of seconds would otherwise be no
 * mark at all while the legend names its family. The segments after it
 * follow it rather than cover it, so a bar may end up to this much past its
 * scale per such segment; the total written at its end is the true one.
 */
const MIN_SEGMENT = 2;

/**
 * What a segment `width` by `height` wide can hold: its node name when the
 * name fits inside it, else its family glyph when that fits, else nothing —
 * the hover box and the table still carry it.
 */
function markFor(s: StackSegment, width: number, height: number): { kind: 'name' } | { kind: 'glyph'; family: Family; size: number } | null {
  if (height >= LABEL_SIZE + 2 && nameWidth(s.label) + 2 * INSET <= width) return { kind: 'name' };
  const size = Math.min(16, height - INSET);
  if (s.family !== null && size >= 10 && size + INSET <= width) return { kind: 'glyph', family: s.family, size };
  return null;
}

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
 * inks — and its total written at its end. A segment wide enough for its node
 * name is labelled with it, a narrower one with its family glyph, so colour is
 * never what alone tells two nodes apart. Hovering a bar's row lists its
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
          // How far the segments drawn at the narrowest width have pushed the rest of the bar.
          let pushed = 0;
          return (
            <g key={bar.id}>
              <text className="rg-chart__group" x={PLOT.left} y={barY(b) - 5}>
                {bar.label}
              </text>
              <g className="rg-chart__stack">
                {(segments[b] ?? []).map((s) => {
                  const from = at;
                  at += s.value;
                  const width = x(at) - x(from);
                  const box = { x: x(from) + pushed, y: barY(b), width: s.value > 0 ? Math.max(width, MIN_SEGMENT) : width, height: thickness };
                  pushed += box.width - width;
                  const mark = markFor(s, box.width, box.height);
                  return (
                    <Fragment key={s.id}>
                      <rect className="rg-chart__seg" data-family={s.family ?? 'none'} {...box} />
                      {mark === null ? null : (
                        // A nested svg clips its content to its own box, the
                        // segment's, so no label can reach a neighbour's.
                        <svg className="rg-chart__seg-label" data-family={s.family ?? 'none'} {...box}>
                          {mark.kind === 'name' ? (
                            <text className="rg-chart__seg-name" x={INSET} y={box.height / 2} dominantBaseline="central">
                              {s.label}
                            </text>
                          ) : (
                            <Glyph name={mark.family} x={(box.width - mark.size) / 2} y={(box.height - mark.size) / 2} width={mark.size} height={mark.size} />
                          )}
                        </svg>
                      )}
                    </Fragment>
                  );
                })}
              </g>
              <text className="rg-chart__total" x={x(totals[b] ?? 0) + pushed + 6} y={barY(b) + thickness / 2} dominantBaseline="middle">
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
