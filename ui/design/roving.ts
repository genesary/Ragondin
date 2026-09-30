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

/**
 * The index that holds the group's one tab stop: the current one, or, when the
 * value matches nothing, the first enabled one, so the group stays reachable.
 */
export function tabStop(disabled: readonly boolean[], current: number): number {
  if (current >= 0) return current;
  const first = disabled.findIndex((d) => !d);
  return first < 0 ? 0 : first;
}

/** +1, -1, or null, for the keys that move along a horizontal group. */
export function arrowStep(key: string): 1 | -1 | null {
  if (key === 'ArrowRight' || key === 'ArrowDown') return 1;
  if (key === 'ArrowLeft' || key === 'ArrowUp') return -1;
  return null;
}
