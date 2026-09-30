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
export function Select({ id, label, options, help, error, className, ...rest }: SelectProps) {
  const line = error ?? help;
  const helpId = `${id}-help`;
  return (
    <div className="rg-field">
      <label className="rg-field__label" htmlFor={id}>
        {label}
      </label>
      <span className={className === undefined ? 'rg-select' : `rg-select ${className}`}>
        <select {...rest} id={id} aria-invalid={error === undefined ? undefined : true} aria-describedby={line === undefined ? undefined : helpId}>
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
