/** @vitest-environment happy-dom */
import { fireEvent, render, screen, within } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { declared } from '../../design/testing/css.ts';
import { DENSE_GRAPH, DENSE_QUERIES, DENSE_TRACE, FAILED_TRACE, HYBRID_GRAPH, HYBRID_QUERIES, HYBRID_TRACE, withPassages } from './fixtures.ts';
import type { ListItem } from './model.ts';
import { ListItemView, NodeInspector, type Side } from './NodeInspector.tsx';
import css from './Replay.css?raw';

const A: Side = { letter: 'A', name: 'hybrid-rerank-gen', graph: HYBRID_GRAPH, trace: HYBRID_TRACE, queries: HYBRID_QUERIES };
const B: Side = { letter: 'B', name: 'dense-only', graph: DENSE_GRAPH, trace: DENSE_TRACE, queries: DENSE_QUERIES };

const item = (over: Partial<ListItem>): ListItem => ({ chunk: 'c5', document: 'd5', grade: 0, score: 0.9, text: 'Passage 5 of the corpus.', rank: 1, former: null, move: null, ...over });
const one = (props: Parameters<typeof ListItemView>[0]) => {
  const { container } = render(
    <ol>
      <ListItemView {...props} />
    </ol>,
  );
  return container.querySelector('li') as HTMLElement;
};

describe('a list item, per state', () => {
  it('gold: a star, and the grade in words, so the star is never the only carrier', () => {
    const li = one({ item: item({ grade: 2 }) });
    expect(li.getAttribute('data-gold')).toBe('true');
    expect(within(li).getByText('★').getAttribute('aria-hidden')).toBe('true');
    expect(within(li).getByText('gold, grade 2')).toBeTruthy();
    const plain = one({ item: item({ grade: 0 }) });
    expect(plain.getAttribute('data-gold')).toBeNull();
    expect(within(plain).queryByText('★')).toBeNull();
  });

  it('ungraded: no star when the API gives no grade — an unjudged query, or a dataset not the run’s', () => {
    const li = one({ item: item({ grade: null }) });
    expect(li.getAttribute('data-gold')).toBeNull();
    expect(within(li).queryByText('★')).toBeNull();
  });

  it('moved up and moved down: an arrow and the former rank, in words', () => {
    const up = one({ item: item({ rank: 1, former: 2, move: 'up' }) });
    expect(up.getAttribute('data-move')).toBe('up');
    expect(up.textContent).toContain('↑');
    expect(within(up).getByText('moved up from rank')).toBeTruthy();
    const down = one({ item: item({ rank: 2, former: 1, move: 'down' }) });
    expect(down.getAttribute('data-move')).toBe('down');
    expect(down.textContent).toContain('↓');
    expect(within(down).getByText('moved down from rank')).toBeTruthy();
  });

  it('discarded: struck through, with its former rank', () => {
    const li = one({ item: item({ rank: 5, former: 5 }), discarded: true });
    expect(li.getAttribute('data-discarded')).toBe('true');
    expect(li.querySelector('del')).toBeTruthy();
    expect(li.textContent).toContain('was rank 5');
    expect(declared(css, '.rg-replay__item[data-discarded] del', 'text-decoration')).toBe('line-through');
  });

  it('no text: the chunk and document ids instead, in mono', () => {
    const li = one({ item: item({ text: null }) });
    expect(li.getAttribute('data-text')).toBe('none');
    expect(within(li).getByText('c5').tagName).toBe('CODE');
    expect(within(li).getByText('d5')).toBeTruthy();
    const text = one({ item: item({}) });
    expect(within(text).getByText('Passage 5 of the corpus.')).toBeTruthy();
  });
});

describe('the inspector, one run', () => {
  it("heads with the node, then its meta line and its metric for this query and over the run", () => {
    render(<NodeInspector node="rerank" from="A" sides={[A]} metric="ndcg@10" />);
    const panel = screen.getByRole('complementary', { name: 'rerank' });
    expect(within(panel).getByText('reranker/cross_encoder')).toBeTruthy();
    expect(within(panel).getByText("349 ms, 35% of this query's time")).toBeTruthy();
    expect(within(panel).getByText('0.8610 on this query; 0.7217 over 2 judged queries')).toBeTruthy();
  });

  it('lists what the reranker kept in rank order with stars, and what it discarded struck through with its former rank', () => {
    render(<NodeInspector node="rerank" from="A" sides={[A]} metric="ndcg@10" />);
    const kept = screen.getByRole('list', { name: 'Ranked by rerank, 4 chunks' });
    expect(within(kept).getAllByRole('listitem').map((li) => li.querySelector('.rg-replay__chunk')?.textContent)).toEqual(['c5', 'c2', 'c4', 'c3']);
    expect(within(kept).getAllByRole('listitem').map((li) => li.getAttribute('data-gold'))).toEqual(['true', 'true', null, null]);
    const discarded = screen.getByRole('list', { name: 'Discarded, 4 chunks' });
    expect(within(discarded).getAllByRole('listitem').map((li) => li.textContent)).toEqual(expect.arrayContaining([expect.stringContaining('was rank 5')]));
  });

  it('shows the context text whole, and the answer text', () => {
    const { container, unmount } = render(<NodeInspector node="context" from="A" sides={[A]} metric="ndcg@10" />);
    expect(container.querySelector('.rg-replay__text')?.textContent).toBe('Passage 5 of the corpus.\nPassage 2 of the corpus.\nPassage 4 of the corpus.');
    expect(screen.getByRole('list', { name: 'Placed by context, 3 chunks' })).toBeTruthy();
    unmount();
    render(<NodeInspector node="answer" from="A" sides={[A]} metric="ndcg@10" />);
    expect(screen.getByText('Tides depend on the moon.')).toBeTruthy();
  });

  it("shows the query's text on the declared input", () => {
    render(<NodeInspector node="question" from="A" sides={[A]} metric="ndcg@10" />);
    expect(screen.getByText('What do tides depend on?')).toBeTruthy();
  });

  it("says a sliver of the query's time as under 1%, never 0%", () => {
    render(<NodeInspector node="rrf" from="A" sides={[A]} metric="ndcg@10" />);
    expect(screen.getByText("1 ms, under 1% of this query's time")).toBeTruthy();
  });

  it('fills the verdict slot for the final node only', () => {
    const { unmount } = render(<NodeInspector node="answer" from="A" sides={[A]} metric="ndcg@10" />);
    expect(screen.getByRole('heading', { name: 'Verdict' })).toBeTruthy();
    expect(screen.getByText('ndcg@10 is 0.8610 on this query, with 2 gold documents in the top 10, the first at document rank 1.')).toBeTruthy();
    unmount();
    render(<NodeInspector node="rerank" from="A" sides={[A]} metric="ndcg@10" />);
    expect(screen.queryByRole('heading', { name: 'Verdict' })).toBeNull();
  });

  it('shows ids rather than text when the passages are not verified', () => {
    render(<NodeInspector node="rerank" from="A" sides={[{ ...A, trace: withPassages(HYBRID_TRACE, 'dataset_absent') }]} metric="ndcg@10" />);
    const kept = screen.getByRole('list', { name: 'Ranked by rerank, 4 chunks' });
    expect(within(kept).getAllByRole('listitem').every((li) => li.getAttribute('data-text') === 'none')).toBe(true);
    expect(within(kept).queryByText('Passage 5 of the corpus.')).toBeNull();
  });

  it('keeps the stars with ids when only the chunk set differs: a grade needs the dataset, not the chunks', () => {
    render(<NodeInspector node="rerank" from="A" sides={[{ ...A, trace: withPassages(HYBRID_TRACE, 'index_differs') }]} metric="ndcg@10" />);
    const kept = screen.getByRole('list', { name: 'Ranked by rerank, 4 chunks' });
    expect(within(kept).getAllByRole('listitem').map((li) => [li.getAttribute('data-gold'), li.getAttribute('data-text')])).toEqual([
      ['true', 'none'],
      ['true', 'none'],
      [null, 'none'],
      [null, 'none'],
    ]);
  });

  it('says a failed node failed, with its error, and a node after it was not run', () => {
    const failed: Side = { ...A, trace: FAILED_TRACE };
    const { unmount } = render(<NodeInspector node="rerank" from="A" sides={[failed]} metric="ndcg@10" />);
    expect(screen.getByText('The service at 127.0.0.1:7001 did not answer within 30 s.')).toBeTruthy();
    expect(screen.getByText(/Failed after 30000 ms/)).toBeTruthy();
    unmount();
    render(<NodeInspector node="answer" from="A" sides={[failed]} metric="ndcg@10" />);
    expect(screen.getByText('Not run: an earlier node failed on this query.')).toBeTruthy();
  });
});

describe('the inspector, side by side', () => {
  it("shows both runs' outputs in two columns, each headed by its run", () => {
    render(<NodeInspector node="dense" from="A" sides={[A, B]} metric="ndcg@10" />);
    const columns = screen.getAllByRole('region');
    expect(columns.map((c) => c.getAttribute('aria-label'))).toEqual(['hybrid-rerank-gen', 'dense-only']);
    expect(within(columns[0]!).getByRole('list', { name: 'Ranked by dense, 5 chunks' })).toBeTruthy();
    expect(within(columns[1]!).getByRole('list', { name: 'Ranked by dense, 5 chunks' })).toBeTruthy();
  });

  it("says there is no such node in the other run, and shows that run's final output instead, labelled", () => {
    render(<NodeInspector node="rerank" from="A" sides={[A, B]} metric="ndcg@10" />);
    const [a, b] = screen.getAllByRole('region') as [HTMLElement, HTMLElement];
    expect(within(a).getByRole('list', { name: 'Ranked by rerank, 4 chunks' })).toBeTruthy();
    expect(within(b).getByText('No such node in dense-only.')).toBeTruthy();
    expect(within(b).getByRole('heading', { name: 'Final output of dense-only: dense' })).toBeTruthy();
    expect(within(b).getByRole('list', { name: 'Ranked by dense, 5 chunks' })).toBeTruthy();
  });

  it('resolves the counterpart from the run the node was selected in', () => {
    render(<NodeInspector node="dense" from="B" sides={[A, B]} metric="ndcg@10" />);
    expect(screen.getAllByRole('region').map((c) => within(c).queryByText(/No such node/))).toEqual([null, null]);
  });

  it("compares the two runs in the verdict on the selected run's final node", () => {
    render(<NodeInspector node="answer" from="A" sides={[A, B]} metric="ndcg@10" />);
    expect(screen.getByText('ndcg@10 is 0.8610 in hybrid-rerank-gen and 0.6131 in dense-only, 0.2479 higher in hybrid-rerank-gen; the first gold document is at document rank 1 in hybrid-rerank-gen and 3 in dense-only.')).toBeTruthy();
  });
});

describe('the verdict slot, side by side', () => {
  it("stays empty on a node that is not its run's final node, even when the other column shows its run's final output", () => {
    render(<NodeInspector node="rerank" from="A" sides={[A, B]} metric="ndcg@10" />);
    expect(screen.queryByRole('heading', { name: 'Verdict' })).toBeNull();
  });
});

describe('a passage, readable in a list', () => {
  afterEach(() => vi.restoreAllMocks());

  it('names a chunk once when it is its whole document, and both when the chunk is a part of it', () => {
    const whole = one({ item: item({ chunk: '4983', document: '4983' }) });
    expect(whole.querySelectorAll('.rg-replay__chunk, .rg-replay__doc')).toHaveLength(1);
    expect(whole.textContent?.match(/4983/g)).toHaveLength(1);
    const part = one({ item: item({ chunk: 'd5#2', document: 'd5' }) });
    expect([...part.querySelectorAll('.rg-replay__chunk, .rg-replay__doc')].map((e) => e.textContent)).toEqual(['d5#2', 'd5']);
    const bare = one({ item: item({ chunk: '77', document: '77', text: null }) });
    expect(bare.textContent?.match(/77/g)).toHaveLength(1);
  });

  it('clamps a long passage to three lines, with a control that shows it whole and back', () => {
    vi.spyOn(HTMLElement.prototype, 'scrollHeight', 'get').mockReturnValue(120);
    vi.spyOn(HTMLElement.prototype, 'clientHeight', 'get').mockReturnValue(54);
    const li = one({ item: item({ text: 'A long passage. '.repeat(40) }) });
    const text = li.querySelector('.rg-replay__clamp') as HTMLElement;
    expect(text.hasAttribute('data-clamped')).toBe(true);
    const more = within(li).getByRole('button', { name: 'Show all' });
    expect(more.getAttribute('aria-expanded')).toBe('false');
    fireEvent.click(more);
    expect(text.hasAttribute('data-clamped')).toBe(false);
    expect(within(li).getByRole('button', { name: 'Show less' }).getAttribute('aria-expanded')).toBe('true');
  });

  it('offers no control for a passage that fits its three lines', () => {
    vi.spyOn(HTMLElement.prototype, 'scrollHeight', 'get').mockReturnValue(36);
    vi.spyOn(HTMLElement.prototype, 'clientHeight', 'get').mockReturnValue(36);
    const li = one({ item: item({ text: 'Short.' }) });
    expect(within(li).queryByRole('button')).toBeNull();
  });

  it('draws the clamp at three lines', () => {
    expect(declared(css, '.rg-replay__clamp[data-clamped]', '-webkit-line-clamp')).toBe('3');
    expect(declared(css, '.rg-replay__clamp[data-clamped]', 'overflow')).toBe('hidden');
  });
});
