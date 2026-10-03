// Replay's query selector: a search over the queries' text, the "where we
// miss" filter, and the queries as one listbox — one tab stop, the arrow
// keys, Home, End and the page keys moving the choice. A benchmark can hold
// ten thousand queries, so the list renders only the rows in view.
// ARCHITECTURE.md § The Replay screen.
import { useEffect, useId, useLayoutEffect, useMemo, useRef, useState, type KeyboardEvent } from 'react';
import { FilterChip, Input } from '../../design/index.ts';
import type { QueryScores } from '../api/types.ts';
import { formatScore, matching } from './model.ts';

/** One row's height and the rows in view: the list's box, fixed, so nothing moves as it scrolls. */
export const ROW = 48;
const IN_VIEW = 10;
const OVERSCAN = 5;

export type QueryFilter = {
  pressed: boolean;
  /** How many queries it keeps, once known. */
  count?: number;
  onToggle: (pressed: boolean) => void;
  disabled?: boolean;
  /** Why it is refused. */
  reason?: string;
};

export type QueryListProps = {
  /** The queries to choose from: all of the run's, or those the filter kept. */
  queries: readonly QueryScores[];
  /** The query shown. */
  current: string;
  /** The metric each query's score is shown on. */
  metric: string | null;
  onChoose: (id: string) => void;
  filter: QueryFilter;
};

const quantity = (n: number) => `${n.toLocaleString('en-US')} quer${n === 1 ? 'y' : 'ies'}`;

/** The index a key moves to from `at` among `n` rows; null for a key that does not move. */
function step(key: string, at: number, n: number): number | null {
  const from = at < 0 ? -1 : at;
  switch (key) {
    case 'ArrowDown':
      return Math.min(n - 1, from + 1);
    case 'ArrowUp':
      return Math.max(0, from - 1);
    case 'Home':
      return 0;
    case 'End':
      return n - 1;
    case 'PageDown':
      return Math.min(n - 1, from + IN_VIEW);
    case 'PageUp':
      return Math.max(0, from - IN_VIEW);
    default:
      return null;
  }
}

export function QueryList({ queries, current, metric, onChoose, filter }: QueryListProps) {
  const base = useId();
  const [search, setSearch] = useState('');
  const [top, setTop] = useState(0);
  const box = useRef<HTMLDivElement>(null);
  const shown = useMemo(() => matching(queries, search), [queries, search]);
  const at = shown.findIndex((q) => q.id === current);

  // The current query is kept in view: a choice made by key scrolls the box
  // to it, and the window follows the box.
  useLayoutEffect(() => {
    if (at < 0) return;
    const first = Math.ceil(top / ROW);
    if (at >= first && at < first + IN_VIEW) return;
    const next = at < first ? at * ROW : (at - IN_VIEW + 1) * ROW;
    setTop(next);
    if (box.current !== null) box.current.scrollTop = next;
  }, [at, top]);
  useEffect(() => {
    // A new search starts at the top of what it found.
    if (box.current !== null) box.current.scrollTop = 0;
    setTop(0);
  }, [search]);

  const start = Math.max(0, Math.floor(top / ROW) - OVERSCAN);
  const end = Math.min(shown.length, start + IN_VIEW + 2 * OVERSCAN);
  const optionId = (i: number) => `${base}-q${i}`;

  const onKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    if (shown.length === 0) return;
    const next = step(event.key, at, shown.length);
    if (next === null) return;
    // The keys move the choice, not the page.
    event.preventDefault();
    if (next !== at) onChoose(shown[next]!.id);
  };

  return (
    <div className="rg-replay__queries">
      <Input id={`${base}-search`} label="Search queries" type="search" value={search} onChange={(e) => setSearch(e.target.value)} placeholder="Words of the question, or its id" />
      <div className="rg-replay__filter">
        {filter.disabled === true ? (
          <FilterChip label="No gold in the top 10" pressed={false} onToggle={filter.onToggle} disabled disabledReason={filter.reason ?? 'Not available'} />
        ) : (
          <FilterChip label="No gold in the top 10" pressed={filter.pressed} onToggle={filter.onToggle} {...(filter.count === undefined ? {} : { count: filter.count })} />
        )}
      </div>
      <p className="rg-replay__count">{shown.length === queries.length ? quantity(queries.length) : `${shown.length.toLocaleString('en-US')} of ${quantity(queries.length)}`}</p>
      <div
        ref={box}
        className="rg-replay__listbox"
        role="listbox"
        aria-label="Queries"
        tabIndex={0}
        aria-activedescendant={at >= start && at < end ? optionId(at) : undefined}
        onKeyDown={onKeyDown}
        onScroll={(e) => setTop(e.currentTarget.scrollTop)}
      >
        <div style={{ height: shown.length * ROW, position: 'relative' }}>
          {shown.slice(start, end).map((q, k) => {
            const i = start + k;
            const score = metric === null ? undefined : q.scores[metric];
            return (
              <div
                key={q.id}
                id={optionId(i)}
                role="option"
                className="rg-replay__option"
                data-query={q.id}
                aria-selected={q.id === current}
                aria-setsize={shown.length}
                aria-posinset={i + 1}
                style={{ top: i * ROW }}
                onClick={() => onChoose(q.id)}
              >
                <span className="rg-replay__qid">{q.id}</span>
                <span className="rg-replay__qscore">{score === undefined ? (Object.keys(q.scores).length === 0 ? 'not judged' : '') : formatScore(score)}</span>
                <span className="rg-replay__qtext">{q.text ?? ''}</span>
              </div>
            );
          })}
        </div>
      </div>
    </div>
  );
}
