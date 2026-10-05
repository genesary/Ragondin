import type { ReactNode } from 'react';
import { Glyph } from '../../glyphs/Glyph.tsx';
import './InlineMessage.css';

export type InlineMessageTone = 'critical' | 'warning' | 'info';

const SPOKEN: Record<InlineMessageTone, string> = { critical: 'Error', warning: 'Warning', info: 'Note' };

export type InlineMessageProps = {
  tone: InlineMessageTone;
  /** What is wrong, in one bold line, with the names and the numbers — words, or words with code spans. */
  title: ReactNode;
  /** Why, or how to fix it. */
  children?: ReactNode;
  /** The fixing action. */
  action?: ReactNode;
  /**
   * Whether it is a live region of its own — an alert when critical, a
   * status when a warning. False inside a live region its caller keeps
   * mounted, so a change is announced once, by that region.
   */
  live?: boolean;
};

/**
 * An error, warning or note placed next to its cause. The glyph names the
 * tone in words for assistive technology; the tint is never the only carrier.
 */
export function InlineMessage({ tone, title, children, action, live = true }: InlineMessageProps) {
  const role = !live ? undefined : tone === 'critical' ? 'alert' : tone === 'warning' ? 'status' : undefined;
  return (
    <div className="rg-inline" data-tone={tone} role={role}>
      <Glyph name="alert" label={SPOKEN[tone]} />
      <div>
        <b>{title}</b>
        {children === undefined ? null : <p>{children}</p>}
      </div>
      {action ?? <span />}
    </div>
  );
}
