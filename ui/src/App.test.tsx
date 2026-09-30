/** @vitest-environment happy-dom */
import { render, screen } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import { App } from './App.tsx';

describe('App', () => {
  it('renders the placeholder page', () => {
    render(<App />);
    expect(screen.getByRole('heading', { level: 1, name: 'Ragondin' })).toBeTruthy();
    expect(screen.getByText(/front end is being built/i)).toBeTruthy();
  });

  // Vitest's globals are off, so Testing Library cannot register its own
  // cleanup; the setup file does. Without it the first test's page would still
  // be mounted here, and `getByRole` would find two headings.
  it('starts from an empty document, the previous render unmounted', () => {
    render(<App />);
    expect(screen.getByRole('heading', { level: 1, name: 'Ragondin' })).toBeTruthy();
  });
});
