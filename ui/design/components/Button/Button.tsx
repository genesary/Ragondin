import { useId, type ButtonHTMLAttributes, type ReactNode } from 'react';
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
  const reasonId = useId();
  const classes = ['rg-btn', `rg-btn--${kind}`, size === 'm' ? '' : `rg-btn--${size}`, className ?? ''].filter(Boolean).join(' ');
  // Disabled is aria-disabled, not the native attribute: the button stays in
  // the tab order, so a keyboard or screen-reader user reaches it and hears
  // why it refuses. The click is refused here instead.
  return (
    <>
      <button
        type="button"
        {...rest}
        className={classes}
        aria-disabled={disabled || undefined}
        aria-describedby={disabled ? reasonId : rest['aria-describedby']}
        title={disabled ? disabledReason : rest.title}
        aria-busy={busy || undefined}
        onClick={busy || disabled ? undefined : onClick}
      >
        {icon === undefined ? null : <Glyph name={icon} />}
        {busy && busyLabel !== undefined ? busyLabel : children}
      </button>
      {/* Outside the button, so the reason describes it rather than joining its name. */}
      {disabled ? (
        <span id={reasonId} className="rg-visually-hidden">
          {disabledReason}
        </span>
      ) : null}
    </>
  );
}
