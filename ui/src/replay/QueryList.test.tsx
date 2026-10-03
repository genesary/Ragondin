/** @vitest-environment happy-dom */
import { fireEvent, render, screen, within } from '@testing-library/react';
import { useState } from 'react';
import { describe, expect, it, vi } from 'vitest';
import type { QueryScores } from '../api/types.ts';
import { HYBRID_QUERIES } from './fixtures.ts';
import { QueryList, type QueryListProps } from './QueryList.tsx';

const FILTER = { pressed: false, onToggle: () => {} };

function renderList(props: Partial<QueryListProps> = {}) {
  const onChoose = vi.fn();
  render(<QueryList queries={HYBRID_QUERIES.queries} current="q1" metric="ndcg@10" onChoose={onChoose} filter={FILTER} {...props} />);
  return { onChoose, list: screen.getByRole('listbox', { name: 'Queries' }) };
}

/** A list that follows its own choice, as the screen does through the address. */
function Following({ queries }: { queries: QueryScores[] }) {
  const [current, setCurrent] = useState(queries[0]!.id);
  return <QueryList queries={queries} current={current} metric="ndcg@10" onChoose={setCurrent} filter={FILTER} />;
}

const many = (n: number): QueryScores[] => Array.from({ length: n }, (_, i) => ({ id: `${i + 1}`, text: `question ${i + 1}`, scores: { 'ndcg@10': 0.5 }, duration_nanos: 1 }));

describe('the query selector', () => {
  it('lists each query by its id and text, with its score on the chosen metric or that it is not judged, the current one selected', () => {
    const { list } = renderList();
    const options = within(list).getAllByRole('option');
    expect(options.map((o) => o.getAttribute('aria-selected'))).toEqual(['true', 'false', 'false']);
    expect(options[0]?.textContent).toContain('What do tides depend on?');
    expect(options[0]?.textContent).toContain('0.8610');
    expect(options[2]?.textContent).toContain('not judged');
    expect(list.getAttribute('aria-activedescendant')).toBe(options[0]?.id);
  });

  it("searches the queries' text", () => {
    const { list } = renderList();
    fireEvent.change(screen.getByLabelText('Search queries'), { target: { value: 'enzyme' } });
    expect(within(list).getAllByRole('option').map((o) => o.getAttribute('data-query'))).toEqual(['q2']);
    expect(screen.getByText('1 of 3 queries')).toBeTruthy();
  });

  it('moves through the queries with the arrow keys, Home and End, each move a choice', () => {
    const { list, onChoose } = renderList();
    fireEvent.keyDown(list, { key: 'ArrowDown' });
    expect(onChoose).toHaveBeenLastCalledWith('q2');
    fireEvent.keyDown(list, { key: 'End' });
    expect(onChoose).toHaveBeenLastCalledWith('q3');
    // At the first query, up and Home stop there rather than wrapping, and choose nothing new.
    fireEvent.keyDown(list, { key: 'ArrowUp' });
    fireEvent.keyDown(list, { key: 'Home' });
    expect(onChoose).toHaveBeenCalledTimes(2);
  });

  it('chooses a query on a click', () => {
    const { list, onChoose } = renderList();
    fireEvent.click(within(list).getAllByRole('option')[1]!);
    expect(onChoose).toHaveBeenCalledWith('q2');
  });

  it('renders a window of a long list, not all of it, and keeps the current query in the window', () => {
    render(<Following queries={many(10_570)} />);
    const list = screen.getByRole('listbox', { name: 'Queries' });
    const options = () => within(list).getAllByRole('option');
    expect(options().length).toBeLessThan(40);
    expect(options()[0]?.getAttribute('aria-setsize')).toBe('10570');
    fireEvent.keyDown(list, { key: 'End' });
    const last = options().find((o) => o.getAttribute('aria-selected') === 'true');
    expect(last?.getAttribute('data-query')).toBe('10570');
    expect(last?.getAttribute('aria-posinset')).toBe('10570');
    expect(list.getAttribute('aria-activedescendant')).toBe(last?.id);
    fireEvent.keyDown(list, { key: 'PageUp' });
    expect(options().find((o) => o.getAttribute('aria-selected') === 'true')?.getAttribute('data-query')).toBe('10560');
  });

  it('offers the "where we miss" filter as a chip with its count, and says what it shows', () => {
    const onToggle = vi.fn();
    renderList({ filter: { pressed: true, count: 1, onToggle } });
    const chip = screen.getByRole('button', { name: /No gold in the top 10/ });
    expect(chip.getAttribute('aria-pressed')).toBe('true');
    expect(chip.textContent).toContain('1');
    fireEvent.click(chip);
    expect(onToggle).toHaveBeenCalledWith(false);
  });

  it('refuses the filter with its reason when the ground truth is not the run’s', () => {
    renderList({ filter: { pressed: false, onToggle: () => {}, disabled: true, reason: 'Needs the run’s own dataset' } });
    expect(screen.getByText('Needs the run’s own dataset')).toBeTruthy();
  });
});
