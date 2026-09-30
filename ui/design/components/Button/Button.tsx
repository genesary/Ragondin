import type { ButtonHTMLAttributes, ReactNode } from 'react';
import { Glyph, type GlyphName } from '../../glyphs/Glyph.tsx';
import './Button.css';

/**
 * primary: the one action per view. secondary: other actions. quiet: toolbars
 * and Undo. destructive: acts at once and offers Undo, so never filled red.
 */
export type ButtonKind = 'primary' | 'secondary' | 'quiet' | 'destructive';

type Disabled = { disabled: true; disabledReason: string } | { disabled?: false; disabledReason?: never };

export type ButtonProps = {
  kind?: ButtonKind;
  /** m is the 32px control; l the one action of an empty state; s dense rows. */
  size?: 's' | 'm' | 'l';
  icon?: GlyphName;
  /** Busy until real progress exists; the label becomes the verb in progress. */
  busy?: boolean;
  busyLabel?: string;
  children: ReactNode;
} & Disabled &
  Omit<ButtonHTMLAttributes<HTMLButtonElement>, 'disabled' | 'children'>;

export function Button({ kind = 'secondary', size = 'm', icon, busy = false, busyLabel, disabled, disabledReason, className, onClick, children, ...rest }: ButtonProps) {
  const classes = ['rg-btn', `rg-btn--${kind}`, size === 'm' ? '' : `rg-btn--${size}`, className ?? ''].filter(Boolean).join(' ');
  return (
    <button
      type="button"
      {...rest}
      className={classes}
      disabled={disabled}
      title={disabled ? disabledReason : rest.title}
      aria-busy={busy || undefined}
      onClick={busy ? undefined : onClick}
    >
      {icon === undefined ? null : <Glyph name={icon} />}
      {busy && busyLabel !== undefined ? busyLabel : children}
    </button>
  );
}
