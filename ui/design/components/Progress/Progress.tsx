import { useId, type ReactNode } from 'react';
import './Progress.css';

export type ProgressState = 'queued' | 'running' | 'done' | 'failed';

export type ProgressProps = {
  state: ProgressState;
  /** Units done, in the benchmark's own noun. Required: there is no indeterminate progress. */
  value: number;
  total: number;
  /** The count in words ("3,982 / 10,570 questions"), or the result a finished run ends on. */
  label: ReactNode;
  /** The estimate, once one is honest. */
  detail?: ReactNode;
  /** The next step: Compare, Open in Replay. */
  action?: ReactNode;
};

const count = (n: number) => n.toLocaleString('en-US');

/**
 * Real progress, counted. It refuses to render without a value rather than
 * falling back to a spinner: work that can be counted is shown counted.
 */
export function Progress({ state, value, total, label, detail, action }: ProgressProps) {
  const countId = useId();
  if (typeof value !== 'number' || !Number.isFinite(value)) throw new Error('Progress needs a finite value: there is no indeterminate variant.');
  if (!Number.isFinite(total) || total <= 0) throw new Error('Progress needs a positive total.');
  const done = Math.min(Math.max(value, 0), total);
  const width = `${Math.round((done / total) * 10000) / 100}%`;
  // The progressbar role is on the track alone: an action beside the count
  // stays a button in the accessibility tree rather than a child of the bar.
  return (
    <div className="rg-progress" data-state={state}>
      <div className="rg-progress__top">
        <span id={countId} className="rg-progress__count">
          {label}
        </span>
        {detail === undefined ? null : <span className="rg-progress__eta">{detail}</span>}
        {action}
      </div>
      <div
        className="rg-progress__track"
        role="progressbar"
        aria-labelledby={countId}
        aria-valuemin={0}
        aria-valuemax={total}
        aria-valuenow={done}
        aria-valuetext={`${state}, ${count(done)} of ${count(total)}`}
      >
        {state === 'queued' ? null : <div className="rg-progress__fill" style={{ width }} />}
      </div>
    </div>
  );
}
