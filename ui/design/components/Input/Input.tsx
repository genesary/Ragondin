import type { InputHTMLAttributes } from 'react';
import { Help } from '../../forms/Help.tsx';
import './Input.css';

export type InputProps = {
  /** Every field has a stable id: labels, helper lines and tests hang off it. */
  id: string;
  /** The parameter's real name, e.g. `top_k`. */
  label: string;
  /** Shown after the value, e.g. "s" or "ms". */
  unit?: string;
  /** Right-aligned with tabular figures. */
  numeric?: boolean;
  /** Parameters, paths, addresses and YAML. */
  mono?: boolean;
  help?: string;
  /** What is wrong and what is allowed, with the numbers. Marks the field invalid. */
  error?: string;
} & Omit<InputHTMLAttributes<HTMLInputElement>, 'id'>;

export function Input({ id, label, unit, numeric = false, mono = false, help, error, className, ...rest }: InputProps) {
  const line = error ?? help;
  const helpId = `${id}-help`;
  const unitId = `${id}-unit`;
  // The unit is part of what the value means ("30" is "30 s"), so it describes the field.
  const describedBy = [unit === undefined ? '' : unitId, line === undefined ? '' : helpId].filter(Boolean).join(' ');
  const classes = ['rg-input', numeric ? 'rg-input--num' : '', mono ? 'rg-input--mono' : '', className ?? ''].filter(Boolean).join(' ');
  const input = (
    <input
      {...rest}
      id={id}
      className={classes}
      aria-invalid={error === undefined ? undefined : true}
      aria-describedby={describedBy === '' ? undefined : describedBy}
    />
  );
  return (
    <div className="rg-field">
      <label className="rg-field__label" htmlFor={id}>
        {label}
      </label>
      {unit === undefined ? (
        input
      ) : (
        <span className="rg-affix">
          {input}
          <span id={unitId} className="rg-affix__unit">
            {unit}
          </span>
        </span>
      )}
      {line === undefined ? null : (
        <Help id={helpId} error={error !== undefined}>
          {line}
        </Help>
      )}
    </div>
  );
}
