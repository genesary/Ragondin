import type { SelectHTMLAttributes } from 'react';
import { Help } from '../../forms/Help.tsx';
import { Glyph } from '../../glyphs/Glyph.tsx';
import './Select.css';

export type SelectOption = { value: string; label: string; disabled?: boolean };

export type SelectProps = {
  id: string;
  label: string;
  options: readonly SelectOption[];
  help?: string;
  error?: string;
} & Omit<SelectHTMLAttributes<HTMLSelectElement>, 'id' | 'children'>;

/** The native select with a drawn chevron: the platform's picker and keyboard stay native. */
export function Select({ id, label, options, help, error, className, 'aria-describedby': described, ...rest }: SelectProps) {
  const line = error ?? help;
  const helpId = `${id}-help`;
  // Its own help line first, then whatever else the caller says describes it — a note beside the field.
  const describedBy = [line === undefined ? null : helpId, described ?? null].filter((part) => part !== null).join(' ');
  return (
    <div className="rg-field">
      <label className="rg-field__label" htmlFor={id}>
        {label}
      </label>
      <span className={className === undefined ? 'rg-select' : `rg-select ${className}`}>
        <select {...rest} id={id} aria-invalid={error === undefined ? undefined : true} aria-describedby={describedBy === '' ? undefined : describedBy}>
          {options.map((o) => (
            <option key={o.value} value={o.value} disabled={o.disabled}>
              {o.label}
            </option>
          ))}
        </select>
        <Glyph name="chevron" />
      </span>
      {line === undefined ? null : (
        <Help id={helpId} error={error !== undefined}>
          {line}
        </Help>
      )}
    </div>
  );
}
