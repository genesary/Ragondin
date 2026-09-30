import type { ReactNode } from 'react';
import { Glyph } from '../glyphs/Glyph.tsx';
import './forms.css';

/**
 * The line under a field: helper text, or the error that says what is wrong
 * and what is allowed. An error carries the alert glyph beside its words, so
 * it never reads by colour alone.
 */
export function Help({ id, error, children }: { id: string; error?: boolean; children: ReactNode }) {
  return (
    <p id={id} className="rg-help" data-error={error ? true : undefined}>
      {error ? <Glyph name="alert" /> : null}
      {children}
    </p>
  );
}
