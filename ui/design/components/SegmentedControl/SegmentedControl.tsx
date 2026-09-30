import { useRef, type KeyboardEvent } from 'react';
import { arrowStep, nextEnabled, tabStop } from '../../roving.ts';
import './SegmentedControl.css';

export type Segment = { value: string; label: string; disabled?: boolean; /** Why it is disabled. */ reason?: string };

export type SegmentedControlProps = {
  /** Names the group for assistive technology. */
  label: string;
  /** Two to five modes of the same view. */
  options: readonly Segment[];
  value: string;
  onChange: (value: string) => void;
};

/**
 * Mutually exclusive modes of one view, switched in place. One tab stop; the
 * arrow keys move the pressed option, skipping a disabled one.
 */
export function SegmentedControl({ label, options, value, onChange }: SegmentedControlProps) {
  const refs = useRef<(HTMLButtonElement | null)[]>([]);
  const disabled = options.map((o) => o.disabled === true);
  const current = options.findIndex((o) => o.value === value);
  const stop = tabStop(disabled, current);
  const onKeyDown = (e: KeyboardEvent) => {
    const step = arrowStep(e.key);
    if (step === null) return;
    // The arrows move the choice, not the page.
    e.preventDefault();
    const next = nextEnabled(disabled, stop, step);
    const option = options[next];
    if (option === undefined) return;
    onChange(option.value);
    refs.current[next]?.focus();
  };
  return (
    <div className="rg-seg" role="radiogroup" aria-label={label}>
      {options.map((o, i) => (
        <button
          key={o.value}
          ref={(el) => {
            refs.current[i] = el;
          }}
          type="button"
          role="radio"
          aria-checked={o.value === value}
          tabIndex={i === stop ? 0 : -1}
          disabled={o.disabled}
          title={o.disabled ? o.reason : undefined}
          onClick={() => onChange(o.value)}
          onKeyDown={onKeyDown}
        >
          {o.label}
        </button>
      ))}
    </div>
  );
}
