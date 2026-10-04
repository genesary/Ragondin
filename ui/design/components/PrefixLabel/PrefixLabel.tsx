import { Glyph } from '../../glyphs/Glyph.tsx';
import './PrefixLabel.css';

/** The words of a prefix label: the parent, when one is named, and the node the prefix stops at. */
export function prefixWords(parent: string | null, upTo: string): string {
  return parent === null ? `prefix up to ${upTo}` : `prefix of ${parent}, up to ${upTo}`;
}

/**
 * A run that is a prefix of a pipeline: "prefix of <parent>, up to <node>",
 * beside the prefix glyph. The words carry the relation; the glyph, hidden
 * from assistive technology, only marks it.
 */
export function PrefixLabel({ parent, upTo }: { parent: string | null; upTo: string }) {
  return (
    <span className="rg-prefix">
      <Glyph name="prefix" />
      {prefixWords(parent, upTo)}
    </span>
  );
}
