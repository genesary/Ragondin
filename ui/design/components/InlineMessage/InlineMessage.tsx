import type { ReactNode } from 'react';
import { Glyph } from '../../glyphs/Glyph.tsx';
import './InlineMessage.css';

export type InlineMessageTone = 'critical' | 'warning' | 'info';

const SPOKEN: Record<InlineMessageTone, string> = { critical: 'Error', warning: 'Warning', info: 'Note' };

export type InlineMessageProps = {
  tone: InlineMessageTone;
  /** What is wrong, in one bold line, with the names and the numbers. */
  title: string;
  /** Why, or how to fix it. */
  children?: ReactNode;
  /** The fixing action. */
  action?: ReactNode;
};

/**
 * An error, warning or note placed next to its cause. The glyph names the
 * tone in words for assistive technology; the tint is never the only carrier.
 */
export function InlineMessage({ tone, title, children, action }: InlineMessageProps) {
  return (
    <div className="rg-inline" data-tone={tone} role={tone === 'critical' ? 'alert' : tone === 'warning' ? 'status' : undefined}>
      <Glyph name="alert" label={SPOKEN[tone]} />
      <div>
        <b>{title}</b>
        {children === undefined ? null : <p>{children}</p>}
      </div>
      {action ?? <span />}
    </div>
  );
}
