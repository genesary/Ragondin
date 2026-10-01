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
  /** The box's accessible name when the visible label alone does not say which item it selects. */
  accessibleLabel?: string;
  /** -1 inside a one-tab-stop group, whose row takes the keyboard; a click still toggles the box. */
  tabIndex?: number;
} & Disabled;

/** A native checkbox whose whole row is the target; disabled, it stays visible and says why. */
export function Checkbox({ label, checked, indeterminate = false, onChange, accessibleLabel, tabIndex, disabled, disabledReason }: CheckboxProps) {
  const ref = useRef<HTMLInputElement>(null);
  const reasonId = useId();
  // `indeterminate` is a DOM property with no attribute, so React cannot set it.
  useEffect(() => {
    if (ref.current !== null) ref.current.indeterminate = indeterminate;
  }, [indeterminate]);
  const box = (
    <label className="rg-check">
      <input
        ref={ref}
        type="checkbox"
        checked={checked}
        disabled={disabled}
        aria-label={accessibleLabel}
        tabIndex={tabIndex}
        aria-describedby={disabled ? reasonId : undefined}
        onChange={(e) => onChange(e.target.checked)}
      />
      <span className="rg-check__label">{label}</span>
    </label>
  );
  if (!disabled) return box;
  // The reason sits beside the label, not in it: it describes the box rather than naming it.
  return (
    <span className="rg-check-wrap">
      {box}
      <span id={reasonId} className="rg-check__reason">
        {disabledReason}
      </span>
    </span>
  );
}
