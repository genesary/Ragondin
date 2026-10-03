import { FAMILY_LABEL, Glyph, type Family } from './Glyph.tsx';

/**
 * The tile for a node, given its family as a configuration spells it
 * (`component:`, and the API's `GraphNode.family`): `context_builder` is the
 * context tile. Null for a family no tile draws — an `extension` node, whose
 * kind is open — so the caller writes its word instead.
 */
const OF_COMPONENT: Readonly<Record<string, Family>> = {
  retriever: 'retriever',
  fusion: 'fusion',
  reranker: 'reranker',
  context_builder: 'context',
  generator: 'generator',
};

export function familyOfComponent(component: string): Family | null {
  return Object.hasOwn(OF_COMPONENT, component) ? (OF_COMPONENT[component] as Family) : null;
}

export type FamilyTileProps = {
  family: Family;
  /** Name the family for assistive technology; leave off when its name is already written beside the tile. */
  labelled?: boolean;
};

/**
 * A family's 24px tile: its pigment, with its glyph in that pigment's own
 * ink, `--on-family-<family>` (control flow takes the query pigment's). The
 * pigment is quiet by design (under 3:1), so the glyph is the second carrier,
 * and a name always sits beside the tile.
 */
export function FamilyTile({ family, labelled = false }: FamilyTileProps) {
  return (
    <span className="rg-tile" data-family={family}>
      {labelled ? <Glyph name={family} label={FAMILY_LABEL[family]} /> : <Glyph name={family} />}
    </span>
  );
}
