// What every screen that lets a person choose a metric opens on: one rule,
// so Pipeline, Compare and Replay open on the same metric over the same
// runs. ARCHITECTURE.md § The Pipeline screen, § The Compare screen, § The
// Replay screen.
import type { MetricFamily } from './api/types.ts';

/**
 * The metric shown first: ndcg@10, else the first `families` calls a ranking
 * metric (`metric_families`, from the API's catalogue — the browser keeps no
 * copy to guess a family from a name), else the first.
 */
export function defaultMetric(metrics: readonly string[], families: Readonly<Record<string, MetricFamily>>): string | null {
  if (metrics.includes('ndcg@10')) return 'ndcg@10';
  return metrics.find((m) => families[m] === 'ranking') ?? metrics[0] ?? null;
}
