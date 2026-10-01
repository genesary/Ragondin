import type { ReactNode } from 'react';
import { Glyph, type GlyphName } from '../../glyphs/Glyph.tsx';
import './StatusChip.css';

export type Status = 'queued' | 'running' | 'done' | 'warning' | 'failed' | 'cancelled';

const GLYPH: Record<Exclude<Status, 'running'>, GlyphName> = { queued: 'clock', done: 'check', warning: 'alert', failed: 'cross', cancelled: 'close' };

type Running = { state: 'running'; /** The real fraction done, 0 to 1. */ fraction: number };
type Other = { state: Exclude<Status, 'running'>; fraction?: never };

export type StatusChipProps = (Running | Other) & {
  /** The specifics, e.g. "queued, 2 ahead" or "failed at <code>rerank</code>"; the state word when absent. */
  children?: ReactNode;
};

/**
 * The state of a run, a node or a service: an icon (or, running, a real
 * meter) and a word on a tinted pill. It reads in grayscale.
 */
export function StatusChip({ state, fraction, children }: StatusChipProps) {
  if (state === 'running') {
    const percent = Math.round(Math.min(Math.max(fraction, 0), 1) * 100);
    return (
      <span className="rg-status" data-state="running">
        <span className="rg-status__meter" aria-hidden="true">
          <i style={{ width: `${percent}%` }} />
        </span>
        {children ?? `running ${percent}%`}
      </span>
    );
  }
  return (
    <span className="rg-status" data-state={state}>
      <Glyph name={GLYPH[state]} />
      {children ?? state}
    </span>
  );
}
