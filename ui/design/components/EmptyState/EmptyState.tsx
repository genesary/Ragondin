import type { ReactNode } from 'react';
import { RankStrip } from '../RankStrip/RankStrip.tsx';
import './EmptyState.css';

export type EmptyStateProps = {
  /** What is missing. */
  heading: string;
  /** One sentence on the default path. */
  children: ReactNode;
  /** The steps to take before the screen can fill, when there are several — a checklist. */
  steps?: ReactNode;
  /** The one action that fills the screen, defaulted to something that works. */
  action?: ReactNode;
  /** At most one other. */
  secondary?: ReactNode;
};

/** A screen before it has data. Its art is the product's own instrument: an empty rank strip. */
export function EmptyState({ heading, children, steps, action, secondary }: EmptyStateProps) {
  return (
    <div className="rg-empty">
      <div className="rg-empty__art" aria-hidden="true">
        <RankStrip hits={[]} large />
      </div>
      <h3>{heading}</h3>
      <p>{children}</p>
      {steps === undefined ? null : <div className="rg-empty__steps">{steps}</div>}
      {action === undefined && secondary === undefined ? null : (
        <div className="rg-empty__actions">
          {action}
          {secondary}
        </div>
      )}
    </div>
  );
}
