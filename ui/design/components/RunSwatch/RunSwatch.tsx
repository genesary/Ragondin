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
  /** Which run the hash button copies, in words ("run A"), when several sit side by side; it names the button. */
  copyLabel?: string;
  /**
   * No letter, seen or heard: for a screen that names its runs by name alone
   * (Replay), where a slot's letter would not be the letter Compare gave the
   * same run. The name then carries the identity, the ink and position beside it.
   */
  unlettered?: boolean;
};

/**
 * A run's identity: a swatch carrying its letter (the baseline, a dashed
 * neutral outline), its name and its hash. Letter, fill and position: never
 * colour alone. Run inks are a separate set from the family pigments.
 */
export function RunSwatch({ slot, name, hash, small = false, onCopyHash, copyLabel, unlettered = false }: RunSwatchProps) {
  const letter = slot === 'base' ? 'baseline' : slot.toUpperCase();
  const hidden = slot === 'base' || small;
  const short = hash?.slice(0, 6);
  // The name holds the characters the button shows, so a person who says
  // what they see reaches it (WCAG 2.5.3, Label in Name).
  const copyName = copyLabel === undefined ? `Copy run hash ${short}` : `Copy ${short}, the hash of ${copyLabel}`;
  return (
    <span className="rg-runlabel">
      <span className={small ? 'rg-swatch rg-swatch--s' : 'rg-swatch'} data-run={slot}>
        {unlettered ? null : hidden ? <span className="rg-visually-hidden">{letter}</span> : letter}
      </span>
      {name === undefined ? null : <span className="rg-runlabel__name">{name}</span>}
      {hash === undefined ? null : (
        <button type="button" className="rg-hash" title={hash} aria-label={copyName} onClick={() => onCopyHash?.(hash)}>
          {short}
        </button>
      )}
    </span>
  );
}
