import type { ReactNode } from 'react';
import { FamilyTile } from '../../glyphs/FamilyTile.tsx';
import { Glyph, type Family } from '../../glyphs/Glyph.tsx';
import './Inspector.css';

export type InspectorProps = {
  family: Family;
  /** The node's name. */
  title: string;
  /** The implementation id, in mono, e.g. `reranker/onnx`. */
  impl?: string;
  /** Over the canvas rather than docked beside it. */
  floating?: boolean;
  onClose?: () => void;
  footer?: ReactNode;
  children: ReactNode;
};

/**
 * The side panel's shell: head (family tile, name, implementation), a
 * scrolling body, and a footer. What fills the body belongs to the screen.
 */
export function Inspector({ family, title, impl, floating = false, onClose, footer, children }: InspectorProps) {
  return (
    <aside className="rg-inspector" data-floating={floating ? true : undefined} aria-label={title}>
      <header className="rg-inspector__head">
        <FamilyTile family={family} labelled />
        <span className="rg-inspector__title">
          <b>{title}</b>
          {impl === undefined ? null : <span className="rg-inspector__impl">{impl}</span>}
        </span>
        {onClose === undefined ? null : (
          <button type="button" className="rg-inspector__close" onClick={onClose}>
            <Glyph name="close" label="Close inspector" />
          </button>
        )}
      </header>
      <div className="rg-inspector__body">{children}</div>
      {footer === undefined ? null : <footer className="rg-inspector__foot">{footer}</footer>}
    </aside>
  );
}
