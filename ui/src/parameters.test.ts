import { describe, expect, it } from 'vitest';
import { bool, float, formatFloat, formatParameter, int, list, str } from './parameters.ts';

describe('a parameter value in words', () => {
  it('draws a float with a fractional part, so it never reads as the integer of the same value', () => {
    expect(formatParameter(float(60))).toBe('60.0');
    expect(formatParameter(int('60'))).toBe('60');
    expect(formatParameter(float(0.2))).toBe('0.2');
    expect(formatParameter(float(-3))).toBe('-3.0');
    expect(formatFloat(1e21)).toBe('1e+21');
    expect(formatFloat(1.5e-7)).toBe('1.5e-7');
  });

  it('keeps an integer whole, however wide', () => {
    expect(formatParameter(int('-9223372036854775808'))).toBe('-9223372036854775808');
  });

  it('quotes text that would read as another kind, and no other', () => {
    expect(formatParameter(str('cross_encoder'))).toBe('cross_encoder');
    expect(formatParameter(str('60'))).toBe('"60"');
    expect(formatParameter(str('true'))).toBe('"true"');
    expect(formatParameter(str(''))).toBe('""');
    expect(formatParameter(str('a, b'))).toBe('"a, b"');
  });

  it('writes a flag and a list as a configuration does', () => {
    expect(formatParameter(bool(true))).toBe('true');
    expect(formatParameter(list(int('1'), str('a'), float(2), bool(false)))).toBe('[1, a, 2.0, false]');
    expect(formatParameter(list())).toBe('[]');
  });
});
