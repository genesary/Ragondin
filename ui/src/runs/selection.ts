// The selection Runs hands to Compare: run ids in the order they were
// checked, all on one benchmark, because a run's metrics are what its
// benchmark's ground truth allows and two benchmarks' figures do not compare
// (ADR-008). The selection lives in the address (`#runs?sel=…`); these are
// the pure rules over it. ARCHITECTURE.md § The Runs screen.
import { benchmarkLabel, type RunRow } from './model.ts';

/** A baseline and up to four runs: Compare refuses a sixth rather than invent a colour (the front-end design, § 3). */
export const COMPARE_CEILING = 5;

/** The selection with `id` checked — appended, so the order is the order of checking — or unchecked. */
export function toggle(sel: readonly string[], id: string): string[] {
  return sel.includes(id) ? sel.filter((s) => s !== id) : [...sel, id];
}

/**
 * Why `row` cannot be checked given `sel`, in a sentence; null when it can.
 * A row already selected can always be unchecked.
 */
export function refusal(row: RunRow, sel: readonly string[], rows: readonly RunRow[]): string | null {
  if (row.status.state === 'failed') return 'A failed run has no metrics to compare.';
  if (row.status.state !== 'done') return 'This run has not finished.';
  if (sel.includes(row.id)) return null;
  const first = rows.find((r) => r.id === sel[0]);
  if (first === undefined || first.benchmark === row.benchmark) return null;
  return `The selected runs are on ${benchmarkLabel(first)}. Compare takes runs on one benchmark.`;
}

/**
 * The part of an address's selection that a person could have checked: ids
 * the listing holds, once each, finished, and on the first one's benchmark.
 */
export function sanitize(sel: readonly string[], rows: readonly RunRow[]): string[] {
  return sel.reduce<string[]>((kept, id) => {
    const row = rows.find((r) => r.id === id);
    return row === undefined || kept.includes(id) || refusal(row, kept, rows) !== null ? kept : [...kept, id];
  }, []);
}

/** Why "Compare" refuses `sel`, in a sentence; null when it can open Compare. */
export function compareRefusal(sel: readonly string[]): string | null {
  if (sel.length < 2) return 'Select at least two runs on one benchmark to compare.';
  if (sel.length > COMPARE_CEILING) return `Compare takes a baseline and up to four runs. Clear ${sel.length - COMPARE_CEILING} to compare.`;
  return null;
}
