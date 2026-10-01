// The selection Runs hands to Compare: run ids in the order they were
// checked, all on one benchmark, because a run's metrics are what its
// benchmark's ground truth allows and two benchmarks' figures do not compare
// (ADR-008); at most five, a baseline and four (the front-end design, § 3).
// The selection lives in the address (`#runs?sel=…`); these are the pure
// rules over it. ARCHITECTURE.md § The Runs screen.
import { benchmarkLabel, runId, type RunRow } from './model.ts';

/** A baseline and up to four runs: Compare refuses a sixth rather than invent a colour (the front-end design, § 3). */
export const COMPARE_CEILING = 5;

/** Why a row cannot be checked: a few words for its box, and the sentence the table says once. */
export type Refusal = { short: string; full: string };

/** The selection with `id` checked — appended, so the order is the order of checking — or unchecked. */
export function toggle(sel: readonly string[], id: string): string[] {
  return sel.includes(id) ? sel.filter((s) => s !== id) : [...sel, id];
}

const findRun = (rows: readonly RunRow[], id: string) => rows.find((r) => runId(r) === id);

/**
 * Why `row` cannot be checked given `sel`; null when it can. A row already
 * selected can always be unchecked. The benchmark is the first selected run
 * the listing holds.
 */
export function refusal(row: RunRow, sel: readonly string[], rows: readonly RunRow[]): Refusal | null {
  if (row.status.state === 'failed') return { short: 'Failed', full: 'A failed run has no metrics to compare.' };
  const id = runId(row);
  if (id === null || row.status.state !== 'done') return { short: 'Not finished', full: 'Only a finished run can be compared.' };
  if (sel.includes(id)) return null;
  const first = sel.map((s) => findRun(rows, s)).find((r) => r !== undefined);
  if (first !== undefined && first.benchmark !== row.benchmark) {
    return { short: 'Other benchmark', full: `The selected runs are on ${benchmarkLabel(first)}. Compare takes runs on one benchmark.` };
  }
  if (sel.length >= COMPARE_CEILING) return { short: 'Five selected', full: 'Compare takes a baseline and up to four runs. Clear one to choose another.' };
  return null;
}

/** The selected ids the listing does not hold, in order. */
export function unknownIds(sel: readonly string[], rows: readonly RunRow[]): string[] {
  return sel.filter((id) => findRun(rows, id) === undefined);
}

/**
 * The part of an address's selection a person could have checked: once
 * each, finished runs on the first one's benchmark, at most five. An id the
 * listing does not hold is kept — the run may be newer than the listing —
 * unless `gone` says a fresh listing confirmed it absent.
 */
export function sanitize(sel: readonly string[], rows: readonly RunRow[], gone: ReadonlySet<string>): string[] {
  return sel.reduce<string[]>((kept, id) => {
    if (kept.includes(id) || kept.length >= COMPARE_CEILING) return kept;
    const row = findRun(rows, id);
    if (row === undefined) return gone.has(id) ? kept : [...kept, id];
    return refusal(row, kept, rows) === null ? [...kept, id] : kept;
  }, []);
}

/** Why "Compare" refuses `sel`, in a sentence; null when it can open Compare. */
export function compareRefusal(sel: readonly string[]): string | null {
  return sel.length < 2 ? 'Select at least two runs on one benchmark to compare.' : null;
}
