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
});
