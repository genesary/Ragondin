import { FAMILY_LABEL, Glyph, type Family } from './Glyph.tsx';

export type FamilyTileProps = {
  family: Family;
  /** Name the family for assistive technology; leave off when its name is already written beside the tile. */
  labelled?: boolean;
};

/**
 * A family's 24px tile: its pigment, with its glyph in `on-family` ink. The
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
