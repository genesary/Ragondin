import { describe, expect, it } from 'vitest';
import { defaultMetric } from './metrics.ts';

describe('the metric a view opens on', () => {
  it("is ndcg@10, else the first metric the API's catalogue calls a ranking metric, else the first", () => {
    expect(defaultMetric(['mrr', 'ndcg@10', 'recall@100'], {})).toBe('ndcg@10');
    expect(defaultMetric(['exact_match', 'recall@5', 'mrr'], { exact_match: 'answers', 'recall@5': 'ranking', mrr: 'ranking' })).toBe('recall@5');
    // A name the listing gives no family is never guessed at from its spelling.
    expect(defaultMetric(['token_f1', 'recall@5'], {})).toBe('token_f1');
    expect(defaultMetric([], {})).toBeNull();
  });
});
