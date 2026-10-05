import { describe, expect, it } from 'vitest';
import { listed } from './words.ts';

describe('listed', () => {
  it('joins names as a sentence lists them', () => {
    expect(listed([])).toBe('');
    expect(listed(['a'])).toBe('a');
    expect(listed(['a', 'b'])).toBe('a and b');
    expect(listed(['a', 'b', 'c'])).toBe('a, b and c');
  });
});
