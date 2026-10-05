/** @vitest-environment happy-dom */
import { render, screen } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import { declared } from '../../design/testing/css.ts';
import { WORKSPACE } from '../editor/fixtures.ts';
import css from './Shell.css?raw';
import { WorkspaceIndicator } from './WorkspaceIndicator.tsx';

const LONG = '/Users/someone/Documents/2.Projects/ragondin/.claude/worktrees/ux/target/demo/workspace';

describe('the workspace indicator on a narrow screen', () => {
  it('keeps the whole path in its name and its title, the count after it', () => {
    render(<WorkspaceIndicator state={{ status: 'loaded', value: { ...WORKSPACE, path: LONG } }} />);
    // Its text is its name: happy-dom's accessible-name reading puts a space after any inline element, so the text is read.
    const link = screen.getByRole('link');
    expect(link.textContent).toBe(`${LONG}, ${WORKSPACE.settings.services.length} services`);
    const path = link.querySelector('.rg-workspace__path') as HTMLElement;
    expect(path.textContent).toBe(LONG);
    // Isolated, so the slash it opens with stays at its start inside a box that runs right to left.
    expect(path.querySelector('bdi')?.textContent).toBe(LONG);
    expect(path.getAttribute('title')).toBe(LONG);
    expect(path.nextSibling?.nodeType).toBe(Node.TEXT_NODE);
  });

  it('cuts a path longer than its line with an ellipsis at its start, keeping the workspace\'s own folder, and never breaks the count', () => {
    // happy-dom lays nothing out: these are the rules; the widths are measured in a browser.
    expect(declared(css, '.rg-workspace', 'min-width')).toBe('0');
    // The count is the link's own text: unbroken, it keeps its width while the path gives way.
    expect(declared(css, '.rg-workspace', 'white-space')).toBe('nowrap');
    expect(declared(css, '.rg-workspace__path', 'overflow')).toBe('hidden');
    expect(declared(css, '.rg-workspace__path', 'text-overflow')).toBe('ellipsis');
    expect(declared(css, '.rg-workspace__path', 'white-space')).toBe('nowrap');
    expect(declared(css, '.rg-workspace__path', 'direction')).toBe('rtl');
    expect(declared(css, '.rg-workspace__path bdi', 'direction')).toBe('ltr');
    // No space between the path and its count beyond the text's own: the gap is the dot's.
    expect(declared(css, '.rg-workspace', 'gap')).toBe('0');
    expect(declared(css, '.rg-workspace > .rg-dot', 'margin-right')).toBe('6px');
  });
});
