import { Glyph } from '../../glyphs/Glyph.tsx';
import './PrefixLabel.css';

/** Names joined as a sentence lists them: "a", "a and b", "a, b and c". */
function listed(names: readonly string[]): string {
  if (names.length <= 1) return names.join('');
  return `${names.slice(0, -1).join(', ')} and ${names[names.length - 1]}`;
}

/** How many parents are named outright before the rest are counted. */
const NAMED = 2;

/**
 * The words of a prefix label: the parents, when any is named, and the node
 * the prefix stops at. Two parents are both named; more read as the first and
 * a count, so a label stays one short phrase however many documents a
 * structural prefix matches.
 */
export function prefixWords(parents: readonly string[], upTo: string): string {
  if (parents.length === 0) return `prefix up to ${upTo}`;
  const named = parents.length <= NAMED ? listed(parents) : `${parents[0]} (and ${parents.length - 1} others)`;
  return `prefix of ${named}, up to ${upTo}`;
}

/**
 * A run that is a prefix of a pipeline: "prefix of <parent>, up to <node>",
 * beside the prefix glyph. The words carry the relation; the glyph, hidden
 * from assistive technology, only marks it. Parents beyond the ones named
 * are in the label's title, for a pointer, and in hidden text, for
 * assistive technology.
 */
export function PrefixLabel({ parents, upTo }: { parents: readonly string[]; upTo: string }) {
  const counted = parents.length > NAMED;
  return (
    <span className="rg-prefix" {...(counted ? { title: `prefix of ${listed(parents)}, up to ${upTo}` } : {})}>
      <Glyph name="prefix" />
      {counted ? (
        <span>
          {`prefix of ${parents[0]} (and ${parents.length - 1} others`}
          <span className="rg-visually-hidden">{`: ${listed(parents.slice(1))}`}</span>
          {`), up to ${upTo}`}
        </span>
      ) : (
        prefixWords(parents, upTo)
      )}
    </span>
  );
}
