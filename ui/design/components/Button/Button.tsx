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

/**
 * The handlers that act — a click, a key, a press, a touch, a submission, in
 * the bubble or the capture phase — which a disabled button must not run.
 * Focus and hover handlers are not among them: a tooltip saying why the button
 * refuses is wired through those, and is needed most while it refuses.
 */
const isActivation = (key: string) => /^on(Click|DoubleClick|KeyDown|KeyUp|PointerDown|PointerUp|MouseDown|MouseUp|TouchStart|TouchEnd|Submit)(Capture)?$/.test(key);

export function Button({ kind = 'secondary', size = 'm', icon, busy = false, busyLabel, disabled, disabledReason, className, onClick, children, ...rest }: ButtonProps) {
  const reasonId = useId();
  const classes = ['rg-btn', `rg-btn--${kind}`, size === 'm' ? '' : `rg-btn--${size}`, className ?? ''].filter(Boolean).join(' ');
  // Disabled is aria-disabled, not the native attribute: the button stays in
  // the tab order, so a keyboard or screen-reader user reaches it and hears
  // why it refuses. aria-disabled stops nothing by itself, so while disabled
  // the button is forced to type="button" (no form submission, by click,
  // Enter or Space) and none of the caller's activation handlers is passed
  // through. Busy, it is forced to type="button" too, so it does not submit
  // twice.
  const passed = disabled ? Object.fromEntries(Object.entries(rest).filter(([key]) => !isActivation(key))) : rest;
  const describedBy = [rest['aria-describedby'], disabled ? reasonId : undefined].filter(Boolean).join(' ');
  return (
    <>
      <button
        type="button"
        {...passed}
        {...(disabled || busy ? { type: 'button' as const } : {})}
        className={classes}
        aria-disabled={disabled || undefined}
        aria-describedby={describedBy === '' ? undefined : describedBy}
        aria-busy={busy || undefined}
        onClick={busy || disabled ? undefined : onClick}
      >
        {icon === undefined ? null : <Glyph name={icon} />}
        {busy && busyLabel !== undefined ? busyLabel : children}
      </button>
      {/* The reason's one accessible path: it describes the button, from
          outside it so it does not join its name. No title as well, which
          would announce it twice. */}
      {disabled ? (
        <span id={reasonId} className="rg-visually-hidden">
          {disabledReason}
        </span>
      ) : null}
    </>
  );
}
