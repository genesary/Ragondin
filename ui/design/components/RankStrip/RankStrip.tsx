import './RankStrip.css';

const K = 10;

export type RankStripProps = {
  /** The ranks, 1-based, at which the gold passages landed; ranks past 10 are not drawn. */
  hits: readonly number[];
  /** Ranks after this one were dropped (a reranker keeping 5); they fade. */
  cut?: number;
  /** 12px cells, for the query header and empty states. */
  large?: boolean;
};

/**
 * The signature figure: ten cells for the top ten ranks, solid where a gold
 * passage landed. Filled versus hollow carries the meaning, so it survives
 * grayscale and every colour-vision type. Never coloured by family or run.
 */
export function RankStrip({ hits, cut = K, large = false }: RankStripProps) {
  const ranks = [...new Set(hits.filter((r) => Number.isInteger(r) && r >= 1 && r <= K))].sort((a, b) => a - b);
  const label = `${ranks.length} gold passage${ranks.length === 1 ? '' : 's'} in the top ${K}${ranks.length > 0 ? `, at rank ${ranks.join(', ')}` : ''}`;
  return (
    <span className={large ? 'rg-rankstrip rg-rankstrip--l' : 'rg-rankstrip'} role="img" aria-label={label} title={label}>
      {Array.from({ length: K }, (_, i) => {
        const rank = i + 1;
        const cls = ranks.includes(rank) ? 'is-hit' : rank > cut ? 'is-cut' : undefined;
        return <i key={rank} className={cls} />;
      })}
    </span>
  );
}
