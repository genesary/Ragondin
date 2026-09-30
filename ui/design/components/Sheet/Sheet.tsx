import { useId, type ReactNode } from 'react';
import './Sheet.css';

/**
 * The card model: one content sheet per screen, the page itself rather than an
 * object on it, split into flush sections by hairlines. It has a border and no
 * shadow; depth is kept for what moves (node cards) and what floats.
 *
 * When not to use one:
 * - not around each chart of a comparison: charts are sections of one sheet;
 * - not around a metric: a number with its label needs no box;
 * - not around list items or run rows: rows are separated by hairlines;
 * - not to group form fields: use a heading and spacing;
 * - not to make something stand out: that budget is spent once per view, by
 *   type or the accent.
 */
export function Sheet({ children }: { children: ReactNode }) {
  return <div className="rg-sheet">{children}</div>;
}

export type SectionProps = {
  /** What the section shows, as a phrase. */
  heading: string;
  /** How to read it. */
  caption?: string;
  /** The heading level the page needs; 2 by default. */
  level?: 2 | 3 | 4;
  children: ReactNode;
};

export function Section({ heading, caption, level = 2, children }: SectionProps) {
  const id = useId();
  const Heading = `h${level}` as const;
  return (
    <section className="rg-section" aria-labelledby={id}>
      <header className="rg-section__head">
        <Heading id={id} className="rg-section__heading">
          {heading}
        </Heading>
        {caption === undefined ? null : <p className="rg-section__caption">{caption}</p>}
      </header>
      {children}
    </section>
  );
}
