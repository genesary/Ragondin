/** @vitest-environment happy-dom */
import { render, screen } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import { declared } from '../../testing/css.ts';
import css from './EmptyState.css?raw';
import { EmptyState } from './EmptyState.tsx';

describe('EmptyState', () => {
  it('names what is missing, says the default path, and offers the one action that fills it', () => {
    render(
      <EmptyState heading="No runs in this workspace yet" action={<button type="button">Run the starter pipeline</button>}>
        The starter pipeline needs no service.
      </EmptyState>,
    );
    expect(screen.getByRole('heading', { name: 'No runs in this workspace yet' })).toBeTruthy();
    expect(screen.getByText('The starter pipeline needs no service.')).toBeTruthy();
    expect(screen.getByRole('button', { name: 'Run the starter pipeline' }).closest('.rg-empty__actions')).toBeTruthy();
    expect(declared(css, '.rg-empty p', 'max-width')).toBe('52ch');
    // The verdict style's role, at 600: a component names a role, never a value.
    expect(declared(css, '.rg-empty h3', 'font')).toBe('var(--type-verdict)');
    expect(declared(css, '.rg-empty h3', 'letter-spacing')).toBe('var(--type-verdict-tracking)');
    expect(declared(css, '.rg-empty h3', 'font-weight')).toBe('600');
  });

  it('draws the product’s own instrument as its art: an empty rank strip', () => {
    const { container } = render(<EmptyState heading="No runs yet">x</EmptyState>);
    const strip = container.querySelector('.rg-empty__art .rg-rankstrip');
    expect(strip).toBeTruthy();
    expect(strip?.querySelectorAll('[data-cell="hit"]')).toHaveLength(0);
    expect(strip?.classList.contains('rg-rankstrip--l')).toBe(true);
  });

  it('holds at most one secondary action beside the primary', () => {
    render(
      <EmptyState heading="No runs yet" action={<button type="button">Run</button>} secondary={<button type="button">Open the Editor</button>}>
        x
      </EmptyState>,
    );
    expect(screen.getAllByRole('button')).toHaveLength(2);
  });
});
