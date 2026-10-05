import { useRef, useState, type DragEvent, type KeyboardEvent } from 'react';
import { FAMILY_LABEL, FamilyTile, familyOfComponent, type Family } from '../../design/index.ts';
import type { Capabilities, ServiceStatus } from '../api/types.ts';
import { DROP_TYPE } from '../canvas/index.ts';
import { Words } from '../words.tsx';

/** One thing the palette offers: a node's `component:` and `impl:`, whether it is a Remote name, and why it cannot be placed, if it cannot. */
export type PaletteEntry = { component: string; impl: string; remote: boolean; refused: string | null };
export type PaletteSection = { label: string; tile: Family; entries: PaletteEntry[] };

// The order a pipeline reads in, which the sections follow. Which families
// appear is the capabilities' to say; this only orders them.
const PIPELINE_ORDER = ['retriever', 'fusion', 'reranker', 'context_builder', 'generator'];
const NO_REMOTE = 'This build cannot call a Remote component: it was built without the remote feature.';

// What M4 cannot place, and the milestone that brings each (ADR-016 § 6;
// docs/OPEN_QUESTIONS.md § 7 for Branch and Loop).
const LATER: PaletteSection[] = [
  { label: 'evaluation', tile: 'judge', entries: [{ component: 'judge', impl: 'judge', remote: false, refused: 'Arrives with M5, the calibrated judge.' }] },
  {
    label: 'control flow',
    tile: 'control',
    entries: ['branch', 'loop'].map((impl) => ({ component: impl, impl, remote: false, refused: 'Arrives with M6, control flow.' })),
  },
];

/**
 * The palette's sections from `GET /workspace`'s capabilities and the
 * services Setup bound: each node family in pipeline order, its local
 * implementations, those this build does not carry — refused, with the
 * reason the capabilities give — then the names bound in it as Remote — refused, with the
 * reason, in a build without `remote` — and the sections that arrive with
 * later milestones. A family no node is (`embedder`) has no section.
 */
export function paletteOf(capabilities: Capabilities, services: readonly ServiceStatus[]): PaletteSection[] {
  const rank = (family: string) => (PIPELINE_ORDER.includes(family) ? PIPELINE_ORDER.indexOf(family) : PIPELINE_ORDER.length);
  const nodeFamilies = capabilities.families.filter((f) => {
    const tile = familyOfComponent(f.family);
    return tile !== null && tile !== 'query';
  });
  const sections = [...nodeFamilies]
    .sort((a, b) => rank(a.family) - rank(b.family))
    .map((f): PaletteSection => {
      const tile = familyOfComponent(f.family)!;
      return {
        label: FAMILY_LABEL[tile],
        tile,
        entries: [
          ...f.parameters.map(({ name }) => ({ component: f.family, impl: name, remote: false, refused: null })),
          ...f.not_carried.map(({ name, reason }) => ({ component: f.family, impl: name, remote: false, refused: `Not in this build: ${reason}.` })),
          ...services.filter((s) => s.family === f.family).map((s) => ({ component: f.family, impl: s.name, remote: true, refused: capabilities.remote ? null : NO_REMOTE })),
        ],
      };
    });
  return [...sections, ...LATER];
}

// An entry's key, unique across the palette: a name may be both carried and bound, and appear in two families.
const keyOf = (entry: PaletteEntry, section: PaletteSection) => `${section.label}/${entry.remote ? 'remote' : 'local'}/${entry.impl}`;

export type PaletteProps = {
  entries: readonly PaletteSection[];
  /** A placeable entry was chosen, by click or by key. */
  onPlace: (component: string, impl: string) => void;
};

/**
 * What can be placed on the canvas: recognition over recall (the front-end
 * design, § 3) — the user picks from what this build runs and never types an
 * `impl:` name. An entry is placed by a click, Enter or Space, or dragged
 * onto the canvas; one that cannot be placed stays visible and focusable, and
 * says why on its second line. Its entries are one tab stop, as a table's rows
 * are, held in one vertical toolbar so a screen reader says so: the up and down arrows — stopping at either end — Home and End move
 * between them, refused ones included, so Tab passes the palette in one press
 * on the way to the canvas.
 */
export function Palette({ entries, onPlace }: PaletteProps) {
  const flat = entries.flatMap((section) => section.entries.map((entry) => keyOf(entry, section)));
  const [active, setActive] = useState<string | null>(null);
  const elements = useRef(new Map<string, HTMLButtonElement>());
  const stop = active !== null && flat.includes(active) ? active : flat[0];
  const onKeyDown = (key: string) => (event: KeyboardEvent<HTMLButtonElement>) => {
    const at = flat.indexOf(key);
    const to = event.key === 'ArrowDown' ? Math.min(at + 1, flat.length - 1) : event.key === 'ArrowUp' ? Math.max(at - 1, 0) : event.key === 'Home' ? 0 : event.key === 'End' ? flat.length - 1 : null;
    if (to === null) return;
    event.preventDefault();
    elements.current.get(flat[to]!)?.focus();
  };
  const drag = (entry: PaletteEntry) => (event: DragEvent<HTMLButtonElement>) => {
    event.dataTransfer.setData(DROP_TYPE, JSON.stringify({ component: entry.component, impl: entry.impl }));
    event.dataTransfer.effectAllowed = 'copy';
  };
  return (
    <section className="rg-palette" aria-label="Palette">
      {/* A vertical toolbar: its role tells a screen reader that the arrow keys reach the entries Tab skips. */}
      <div className="rg-palette__entries" role="toolbar" aria-orientation="vertical" aria-label="Palette entries">
      {entries.map((section) => (
        <div key={section.label} className="rg-palette__section" role="group" aria-label={section.label}>
          <h3 className="rg-palette__head">
            <FamilyTile family={section.tile} />
            {section.label}
          </h3>
          {section.entries.length === 0 ? <p className="rg-palette__none">None in this build.</p> : null}
          {section.entries.map((entry) => {
            const key = keyOf(entry, section);
            return (
              <button
                key={key}
                ref={(el) => {
                  if (el === null) elements.current.delete(key);
                  else elements.current.set(key, el);
                }}
                type="button"
                tabIndex={key === stop ? 0 : -1}
                onFocus={() => setActive(key)}
                onKeyDown={onKeyDown(key)}
                className="rg-palette__entry"
                draggable={entry.refused === null}
                aria-disabled={entry.refused === null ? undefined : true}
                onDragStart={entry.refused === null ? drag(entry) : undefined}
                onClick={() => {
                  if (entry.refused === null) onPlace(entry.component, entry.impl);
                }}
              >
                <b>{entry.impl}</b>
                {entry.remote ? <span className="rg-palette__tag">Remote</span> : null}
                {entry.refused === null ? null : (
                  <small>
                    <Words text={entry.refused} />
                  </small>
                )}
              </button>
            );
          })}
        </div>
      ))}
      </div>
    </section>
  );
}
