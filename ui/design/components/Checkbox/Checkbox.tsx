import { useEffect, useId, useRef } from 'react';
import './Checkbox.css';

type Disabled = { disabled: true; disabledReason: string } | { disabled?: false; disabledReason?: never };

export type CheckboxProps = {
  /** A run's name, or an option in plain words. */
  label: string;
  checked: boolean;
  /** Some, not all, of a group is selected. */
  indeterminate?: boolean;
  onChange: (checked: boolean) => void;
} & Disabled;

/** A native checkbox whose whole row is the target; disabled, it stays visible and says why. */
export function Checkbox({ label, checked, indeterminate = false, onChange, disabled, disabledReason }: CheckboxProps) {
  const ref = useRef<HTMLInputElement>(null);
  const reasonId = useId();
  // `indeterminate` is a DOM property with no attribute, so React cannot set it.
  useEffect(() => {
    if (ref.current !== null) ref.current.indeterminate = indeterminate;
  }, [indeterminate]);
  return (
    <label className="rg-check">
      <input
        ref={ref}
        type="checkbox"
        checked={checked}
        disabled={disabled}
        aria-describedby={disabled ? reasonId : undefined}
        onChange={(e) => onChange(e.target.checked)}
      />
      <span className="rg-check__label">{label}</span>
      {disabled ? (
        <span id={reasonId} className="rg-check__reason">
          {disabledReason}
        </span>
      ) : null}
    </label>
  );
}
