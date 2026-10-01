// The geometry every chart shares: one linear scale per chart (the design
// system's data rule — never two axes) and the round ticks along it.
import type { RunSlot } from '../components/RunSwatch/RunSwatch.tsx';

/**
 * The drawing box every chart is laid out in, in SVG user units. The SVG is
 * drawn at most this wide in CSS pixels (Charts.css), so its labels keep the
 * type scale's size, and narrower on a narrow screen, where it scales down.
 */
export const PLOT = { width: 800, height: 240, top: 24, right: 8, bottom: 44, left: 40 } as const;

/** A run as a chart series: its ink is its slot in the comparison, `short` the letter written beside its mark. */
export type RunSeries = { id: string; label: string; short: string; ink: RunSlot };

/** A linear map from `domain` onto `range`; a flat domain maps to the range's start. */
export function linear([d0, d1]: readonly [number, number], [r0, r1]: readonly [number, number]): (value: number) => number {
  const span = d1 - d0;
  if (span === 0) return () => r0;
  return (value) => r0 + ((value - d0) / span) * (r1 - r0);
}

/**
 * About `count` round ticks across `domain` — a step of 1, 2 or 5 times a
 * power of ten — from its start, never past its end.
 */
export function ticks([d0, d1]: readonly [number, number], count: number): number[] {
  const span = d1 - d0;
  if (span <= 0 || count < 1) return [d0];
  const raw = span / count;
  const power = 10 ** Math.floor(Math.log10(raw));
  const step = ([1, 2, 5, 10].find((m) => m * power >= raw) ?? 10) * power;
  const out: number[] = [];
  // Rounded to the step's own precision, so 0.6000000000000001 prints as 0.6.
  const digits = Math.max(0, -Math.floor(Math.log10(step)));
  for (let k = 0; d0 + k * step <= d1 + step * 1e-9; k++) out.push(Number((d0 + k * step).toFixed(digits)));
  return out;
}
