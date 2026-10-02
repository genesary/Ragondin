import { useRef, useState, type KeyboardEvent } from 'react';
import { Glyph } from '../glyphs/Glyph.tsx';
import { arrowStep } from '../roving.ts';
import './Charts.css';

export type HistogramBin = {
  id: string;
  /** The bin in words, e.g. "much worse". */
  label: string;
  /** Its bounds in words, e.g. "below −0.3": the caller's, never computed here. */
  range: string;
  count: number;
  /** Which arm it sits on: the reserved worse and better colours, or the neutral middle. */
  tone: 'worse' | 'zero' | 'better';
};

export type HistogramProps = {
  /** Names the group of bars. */
  label: string;
  /** In order along the axis, the worse arm first. */
  bins: readonly HistogramBin[];
  /** The two halves' names, written under the axis. */
  halves: { worse: string; better: string };
  /** The bin whose contents are open, if any. */
  active: string | null;
  /** A bar was clicked, or Enter or Space pressed on it. */
  onActivate: (id: string) => void;
  /** The id of what an open bar shows — its list. */
  controls: string;
};

const queries = (n: number) => `${n.toLocaleString('en-US')} quer${n === 1 ? 'y' : 'ies'}`;

/**
 * A diverging histogram: the worse arm, the neutral middle, the better arm,
 * each bar a button the height of its column — its hover and click target
 * larger than the bar itself — named by its bin, its range and its count.
 * The bars are one tab stop, moved along by the arrow keys, Home and End.
 */
export function Histogram({ label, bins, halves, active, onActivate, controls }: HistogramProps) {
  const max = Math.max(0, ...bins.map((b) => b.count));
  const activeAt = bins.findIndex((b) => b.id === active);
  const [focused, setFocused] = useState<number | null>(null);
  const stop = focused ?? (activeAt >= 0 ? activeAt : 0);
  const buttons = useRef<(HTMLButtonElement | null)[]>([]);

  const move = (to: number) => {
    setFocused(to);
    buttons.current[to]?.focus();
  };
  const onKeyDown = (i: number) => (event: KeyboardEvent<HTMLButtonElement>) => {
    const last = bins.length - 1;
    const step = arrowStep(event.key);
    // The arrows stop at the ends rather than wrap: an axis has two ends.
    const to = step !== null ? Math.min(Math.max(i + step, 0), last) : event.key === 'Home' ? 0 : event.key === 'End' ? last : null;
    if (to === null) return;
    event.preventDefault();
    move(to);
  };

  return (
    <div className="rg-hist">
      <div className="rg-hist__bins" role="group" aria-label={label}>
        {bins.map((bin, i) => (
          <button
            key={bin.id}
            ref={(el) => {
              buttons.current[i] = el;
            }}
            type="button"
            className="rg-hist__col"
            aria-label={`${bin.label}, ${bin.range}: ${queries(bin.count)}`}
            aria-expanded={bin.id === active}
            aria-controls={controls}
            tabIndex={i === stop ? 0 : -1}
            onFocus={() => setFocused(i)}
            onKeyDown={onKeyDown(i)}
            onClick={() => onActivate(bin.id)}
          >
            <span className="rg-hist__count">{bin.count.toLocaleString('en-US')}</span>
            <span className="rg-hist__track">
              <span className="rg-hist__bar" data-tone={bin.tone} style={{ height: `${max === 0 ? 0 : (bin.count / max) * 100}%` }} />
            </span>
            <span className="rg-hist__range">{bin.range}</span>
          </button>
        ))}
      </div>
      <div className="rg-hist__halves">
        <span className="rg-hist__half">
          <Glyph name="down" />
          {halves.worse}
        </span>
        <span className="rg-hist__half">
          {halves.better}
          <Glyph name="up" />
        </span>
      </div>
    </div>
  );
}
