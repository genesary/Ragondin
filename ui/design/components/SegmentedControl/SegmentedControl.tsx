import { useRef } from 'react';
import { arrowStep, nextEnabled } from '../../roving.ts';
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
  const current = options.findIndex((o) => o.value === value);
  const onKeyDown = (key: string) => {
    const step = arrowStep(key);
    if (step === null) return;
    const next = nextEnabled(options.map((o) => o.disabled === true), current, step);
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
          tabIndex={o.value === value ? 0 : -1}
          disabled={o.disabled}
          title={o.disabled ? o.reason : undefined}
          onClick={() => onChange(o.value)}
          onKeyDown={(e) => onKeyDown(e.key)}
        >
          {o.label}
        </button>
      ))}
    </div>
  );
}
