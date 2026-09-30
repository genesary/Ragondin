/**
 * The next enabled index from `from`, moving by `step` and wrapping: the
 * arrow-key rule a one-tab-stop group follows. Returns `from` when nothing
 * else is enabled.
 */
export function nextEnabled(disabled: readonly boolean[], from: number, step: 1 | -1): number {
  const n = disabled.length;
  for (let k = 1; k <= n; k++) {
    const i = (((from + step * k) % n) + n) % n;
    if (!disabled[i]) return i;
  }
  return from;
}

/** +1, -1, or null, for the keys that move along a horizontal group. */
export function arrowStep(key: string): 1 | -1 | null {
  if (key === 'ArrowRight' || key === 'ArrowDown') return 1;
  if (key === 'ArrowLeft' || key === 'ArrowUp') return -1;
  return null;
}
