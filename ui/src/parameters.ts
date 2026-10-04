// A parameter value, one rule for every screen: the API's tagged
// `ParameterValue` (ADR-C40 § 2), built and drawn here. Its kind is part of
// the pipeline — `60` and `60.0` hash apart (ADR-C22) — so nothing here ever
// draws two kinds alike.
import type { ParameterValue } from './api/types.ts';

/** An integer, as the decimal text it travels as: the browser's number cannot hold every 64-bit one. */
export const int = (value: string): ParameterValue => ({ kind: 'int', value });
/** A float; finite, or the server refuses it. */
export const float = (value: number): ParameterValue => ({ kind: 'float', value });
export const str = (value: string): ParameterValue => ({ kind: 'string', value });
export const bool = (value: boolean): ParameterValue => ({ kind: 'bool', value });
export const list = (...value: ParameterValue[]): ParameterValue => ({ kind: 'list', value });

/** A float with a fractional part, `60.0`, so it never reads as the integer `60`. */
export function formatFloat(value: number): string {
  const text = String(value);
  return Number.isInteger(value) && !text.includes('e') ? `${text}.0` : text;
}

// Text a reader would take for a number, a flag, a list, quoted text or
// nothing at all: spaces at its ends hide, so ` 60` is checked as `60`, and a
// leading bracket or quote reads as a list or as quoted text.
const LOOKS_TYPED = /^\s*$|^\s*[-+]?(\d+\.?\d*|\.\d+)([eE][-+]?\d+)?\s*$|^\s*(true|false)\s*$|,|^\s*["'[]/;

/** A value as a configuration writes it; text that would read as another kind in quotes. */
export function formatParameter(value: ParameterValue): string {
  switch (value.kind) {
    case 'int':
      return value.value;
    case 'float':
      return formatFloat(value.value);
    case 'bool':
      return String(value.value);
    case 'string':
      return LOOKS_TYPED.test(value.value) ? JSON.stringify(value.value) : value.value;
    case 'list':
      return `[${value.value.map(formatParameter).join(', ')}]`;
  }
}
