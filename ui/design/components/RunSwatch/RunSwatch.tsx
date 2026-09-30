import './RunSwatch.css';

/** The run's slot in the current comparison: the baseline, then candidates A to D. */
export type RunSlot = 'base' | 'a' | 'b' | 'c' | 'd';

export type RunSwatchProps = {
  slot: RunSlot;
  /** The pipeline's name. */
  name?: string;
  /** The full content hash; six characters are shown. */
  hash?: string;
  /** The 10px swatch of legends and table headers; the letter drops from sight there. */
  small?: boolean;
  /** Called with the full hash when the hash label is clicked. */
  onCopyHash?: (hash: string) => void;
};

/**
 * A run's identity: a swatch carrying its letter (the baseline, a dashed
 * neutral outline), its name and its hash. Letter, fill and position: never
 * colour alone. Run inks are a separate set from the family pigments.
 */
export function RunSwatch({ slot, name, hash, small = false, onCopyHash }: RunSwatchProps) {
  const letter = slot === 'base' ? 'baseline' : slot.toUpperCase();
  const hidden = slot === 'base' || small;
  return (
    <span className="rg-runlabel">
      <span className={small ? 'rg-swatch rg-swatch--s' : 'rg-swatch'} data-run={slot}>
        {hidden ? <span className="rg-visually-hidden">{letter}</span> : letter}
      </span>
      {name === undefined ? null : <span className="rg-runlabel__name">{name}</span>}
      {hash === undefined ? null : (
        <button type="button" className="rg-hash" title={hash} aria-label={`Copy run hash ${hash}`} onClick={() => onCopyHash?.(hash)}>
          {hash.slice(0, 6)}
        </button>
      )}
    </span>
  );
}
