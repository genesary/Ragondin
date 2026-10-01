import { FAMILIES, FAMILY_LABEL, FamilyTile } from '../../design/index.ts';
import type { CanvasModel } from './model.ts';
import { PORT_KINDS, PORT_LABEL, PortSwatch } from './Port.tsx';

/**
 * Every hue and every port shape on the canvas, explained: each family drawn,
 * with its tile and its name, then each port shape drawn, with its word —
 * and whether the positions are the canvas's own rather than stored ones.
 */
export function Legend({ model, autoPlaced }: { model: CanvasModel; autoPlaced: number }) {
  const families = FAMILIES.filter((f) => model.nodes.some((n) => n.family === f));
  const kinds = PORT_KINDS.filter((k) => model.nodes.some((n) => n.output === k || n.inputs.includes(k)));
  return (
    <div className="rg-canvas__legend" aria-label="Legend" role="group">
      {families.map((family) => (
        <span key={family} data-legend="family">
          <FamilyTile family={family} />
          {FAMILY_LABEL[family]}
        </span>
      ))}
      {kinds.map((kind) => (
        <span key={kind} data-legend="port">
          <PortSwatch kind={kind} />
          {PORT_LABEL[kind]}
        </span>
      ))}
      {autoPlaced === 0 ? null : (
        <span data-legend="layout">
          {autoPlaced === model.nodes.length ? 'Laid out automatically' : `${autoPlaced} node${autoPlaced === 1 ? '' : 's'} placed automatically`}
        </span>
      )}
    </div>
  );
}
