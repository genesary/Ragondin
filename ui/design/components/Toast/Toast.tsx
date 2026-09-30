import { useEffect, useRef, useState, type ReactNode } from 'react';
import { Glyph } from '../../glyphs/Glyph.tsx';
import './Toast.css';

/** How long a toast stays when nobody is reading it. */
export const TOAST_MS = 6000;

export type ToastProps = {
  /** good: a confirmation. critical: a refusal whose cause is on another screen. */
  tone?: 'good' | 'critical';
  /** The past-tense result, in the action's own verb: "Deleted node rerank". */
  children: ReactNode;
  /** Undo, or the link to the cause. */
  action?: { label: string; onClick: () => void };
  /** Called when the toast leaves: after six unattended seconds, or by hand. */
  onDismiss?: () => void;
};

/**
 * A short confirmation of what just happened. It pauses while hovered or
 * while something in it has focus, so it can be read and reached.
 */
export function Toast({ tone = 'good', children, action, onDismiss }: ToastProps) {
  const [hovered, setHovered] = useState(false);
  const [focused, setFocused] = useState(false);
  const dismiss = useRef(onDismiss);
  useEffect(() => {
    dismiss.current = onDismiss;
  }, [onDismiss]);
  useEffect(() => {
    if (hovered || focused) return undefined;
    const timer = setTimeout(() => dismiss.current?.(), TOAST_MS);
    return () => clearTimeout(timer);
  }, [hovered, focused]);

  return (
    <div
      className="rg-toast"
      data-tone={tone}
      role={tone === 'critical' ? 'alert' : 'status'}
      onMouseEnter={() => setHovered(true)}
      onMouseLeave={() => setHovered(false)}
      onFocus={() => setFocused(true)}
      onBlur={() => setFocused(false)}
    >
      <Glyph name={tone === 'critical' ? 'alert' : 'check'} />
      <span className="rg-toast__msg">{children}</span>
      <span className="rg-toast__actions">
        {action === undefined ? null : (
          <button type="button" className="rg-toast__action" onClick={action.onClick}>
            {action.label}
          </button>
        )}
        {onDismiss === undefined ? null : (
          <button type="button" className="rg-toast__close" onClick={onDismiss}>
            <Glyph name="close" label="Dismiss" />
          </button>
        )}
      </span>
    </div>
  );
}
