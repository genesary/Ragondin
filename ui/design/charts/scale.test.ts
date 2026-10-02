import { describe, expect, it } from 'vitest';
import { linear, ticks } from './scale.ts';

describe('linear', () => {
  it('maps the domain onto the range, either way round', () => {
    const y = linear([0, 1], [200, 0]);
    expect(y(0)).toBe(200);
    expect(y(1)).toBe(0);
    expect(y(0.25)).toBe(150);
  });

  it('maps a flat domain to the start of the range rather than dividing by zero', () => {
    expect(linear([3, 3], [0, 100])(3)).toBe(0);
  });
});

describe('ticks', () => {
  it('steps a 0–1 domain in fifths', () => {
    expect(ticks([0, 1], 5)).toEqual([0, 0.2, 0.4, 0.6, 0.8, 1]);
  });

  it('picks a round step for an arbitrary domain, never past its end', () => {
    expect(ticks([0, 37], 4)).toEqual([0, 10, 20, 30]);
  });

  it('gives one tick for a flat domain', () => {
    expect(ticks([0, 0], 5)).toEqual([0]);
  });
});
