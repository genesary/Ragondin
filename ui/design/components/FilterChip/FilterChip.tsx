import { useId } from 'react';
import { Glyph } from '../../glyphs/Glyph.tsx';
import './FilterChip.css';

type Disabled = { disabled: true; disabledReason: string } | { disabled?: false; disabledReason?: never };

export type FilterChipProps = {
  label: string;
  /** What the filter would show, so nobody clicks into an empty list. */
  count?: number;
  pressed: boolean;
  onToggle: (pressed: boolean) => void;
} & Disabled;

/** A toggle filter above a list. Pressed changes shape (a check) as well as fill. */
export function FilterChip({ label, count, pressed, onToggle, disabled, disabledReason }: FilterChipProps) {
  const reasonId = useId();
  const chip = (
    <button
      type="button"
      className="rg-filter"
      aria-pressed={pressed}
      disabled={disabled}
      aria-describedby={disabled ? reasonId : undefined}
      onClick={() => onToggle(!pressed)}
    >
      {pressed ? <Glyph name="check" /> : null}
      {label}
      {count === undefined ? null : <span className="rg-count">{count.toLocaleString('en-US')}</span>}
    </button>
  );
  if (!disabled) return chip;
  // The reason describes the chip; inside it, it would become part of its name.
  return (
    <span className="rg-filter-wrap">
      {chip}
      <span id={reasonId} className="rg-filter__reason">
        {disabledReason}
      </span>
    </span>
  );
}
