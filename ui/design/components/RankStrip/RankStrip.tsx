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

/** The gold ranks the strip draws: whole ranks in the top ten, once each, in order. */
const drawn = (hits: readonly number[]) => [...new Set(hits.filter((r) => Number.isInteger(r) && r >= 1 && r <= K))].sort((a, b) => a - b);

/**
 * The strip's sentence, its second carrier: one definition, so a node
 * described elsewhere — the canvas's accessible description — reads the same.
 */
export function rankSentence(hits: readonly number[]): string {
  const ranks = drawn(hits);
  return `${ranks.length} gold passage${ranks.length === 1 ? '' : 's'} in the top ${K}${ranks.length > 0 ? `, at rank ${ranks.join(', ')}` : ''}`;
}

/**
 * The signature figure: ten cells for the top ten ranks, solid where a gold
 * passage landed. Filled versus hollow carries the meaning, so it survives
 * grayscale and every colour-vision type. Never coloured by family or run.
 */
export function RankStrip({ hits, cut = K, large = false }: RankStripProps) {
  const ranks = drawn(hits);
  const label = rankSentence(hits);
  return (
    <span className={large ? 'rg-rankstrip rg-rankstrip--l' : 'rg-rankstrip'} role="img" aria-label={label} title={label}>
      {Array.from({ length: K }, (_, i) => {
        const rank = i + 1;
        const cell = ranks.includes(rank) ? 'hit' : rank > cut ? 'cut' : 'miss';
        return <i key={rank} data-cell={cell} />;
      })}
    </span>
  );
}
