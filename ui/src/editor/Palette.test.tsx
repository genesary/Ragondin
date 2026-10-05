/** @vitest-environment happy-dom */
import { act, fireEvent, render, screen, within } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import { DROP_TYPE } from '../canvas/index.ts';
import { SERVICES, WORKSPACE } from './fixtures.ts';
import { Palette, paletteOf } from './Palette.tsx';
import { byWords } from '../words.testing.ts';

const renderPalette = (remote = true) => {
  const onPlace = vi.fn();
  render(<Palette entries={paletteOf({ ...WORKSPACE.capabilities, remote }, SERVICES.services)} onPlace={onPlace} />);
  return { onPlace, palette: screen.getByRole('region', { name: 'Palette' }) };
};
const section = (palette: HTMLElement, name: string) => within(palette).getByRole('group', { name });
const entry = (palette: HTMLElement, name: RegExp) => within(palette).getByRole('button', { name });

describe('the palette, from this build’s capabilities', () => {
  it('has one section per node family in pipeline order, then evaluation and control flow', () => {
    const { palette } = renderPalette();
    expect(within(palette).getAllByRole('group').map((g) => g.getAttribute('aria-label'))).toEqual(['retriever', 'fusion', 'reranker', 'context builder', 'generator', 'evaluation', 'control flow']);
  });

  it('lists exactly the local implementations the capabilities report, and no embedder, which no node is', () => {
    const { palette } = renderPalette();
    const placeable = (name: string) => within(section(palette, name)).getAllByRole('button').filter((b) => b.getAttribute('aria-disabled') === null).map((b) => b.querySelector('b')?.textContent);
    expect(placeable('retriever')).toEqual(['bm25', 'dense']);
    expect(placeable('fusion')).toEqual(['rrf']);
    expect(placeable('reranker')).toEqual([]);
    expect(placeable('context builder')).toEqual(['concat']);
    expect(within(palette).queryAllByRole('button').map((b) => b.querySelector('b')?.textContent)).not.toContain('onnx');
    expect(within(palette).queryByText('bge')).toBeNull();
  });

  it('marks a Remote name bound in Setup as such, and places it', () => {
    const { palette, onPlace } = renderPalette();
    const qwen = entry(palette, /^qwen/);
    expect(within(qwen).getByText('Remote')).toBeTruthy();
    expect(qwen.getAttribute('aria-disabled')).toBeNull();
    fireEvent.click(qwen);
    expect(onPlace).toHaveBeenCalledWith('generator', 'qwen');
  });

  it('greys a Remote name a build without `remote` cannot call, with the reason, and places nothing', () => {
    const { palette, onPlace } = renderPalette(false);
    const qwen = entry(palette, /^qwen/);
    expect(qwen.getAttribute('aria-disabled')).toBe('true');
    expect(within(qwen).getByText('This build cannot call a Remote component: it was built without the remote feature.')).toBeTruthy();
    fireEvent.click(qwen);
    expect(onPlace).not.toHaveBeenCalled();
  });

  it('says so of a family with nothing to place', () => {
    const onPlace = vi.fn();
    const capabilities = { ...WORKSPACE.capabilities, families: WORKSPACE.capabilities.families.map((f) => (f.family === 'generator' ? { ...f, not_carried: [] } : f)) };
    render(<Palette entries={paletteOf(capabilities, [])} onPlace={onPlace} />);
    expect(within(screen.getByRole('group', { name: 'generator' })).getByText('None in this build.')).toBeTruthy();
  });

  it('greys an implementation this build does not carry, with the reason the capabilities give, and places nothing', () => {
    const { palette, onPlace } = renderPalette();
    const crossEncoder = entry(palette, /^cross_encoder/);
    expect(crossEncoder.getAttribute('aria-disabled')).toBe('true');
    expect(within(crossEncoder).getByText(byWords('Not in this build: needs the `onnx` feature.'))).toBeTruthy();
    const stub = entry(palette, /^stub_generator/);
    expect(stub.getAttribute('aria-disabled')).toBe('true');
    expect(within(stub).getByText(byWords('Not in this build: needs the `stub` feature.'))).toBeTruthy();
    expect(stub.getAttribute('draggable')).toBe('false');
    fireEvent.click(stub);
    expect(onPlace).not.toHaveBeenCalled();
  });

  it('shows the judge, Branch and Loop as non-placeable, saying which milestone brings them', () => {
    const { palette, onPlace } = renderPalette();
    for (const [name, line] of [
      [/^judge/, 'Arrives with M5, the calibrated judge.'],
      [/^branch/, 'Arrives with M6, control flow.'],
      [/^loop/, 'Arrives with M6, control flow.'],
    ] as const) {
      const button = entry(palette, name);
      expect(button.getAttribute('aria-disabled')).toBe('true');
      expect(within(button).getByText(line)).toBeTruthy();
      fireEvent.click(button);
    }
    expect(onPlace).not.toHaveBeenCalled();
  });

  it('places a local implementation on click, and hands it to a drag for the canvas to take', () => {
    const { palette, onPlace } = renderPalette();
    fireEvent.click(entry(palette, /^rrf/));
    expect(onPlace).toHaveBeenCalledWith('fusion', 'rrf');
    const data = new Map<string, string>();
    fireEvent.dragStart(entry(palette, /^bm25/), { dataTransfer: { setData: (type: string, value: string) => data.set(type, value), effectAllowed: 'none' } });
    expect(JSON.parse(data.get(DROP_TYPE)!)).toEqual({ component: 'retriever', impl: 'bm25' });
  });

  it('makes a non-placeable entry impossible to drag', () => {
    const { palette } = renderPalette();
    expect(entry(palette, /^judge/).getAttribute('draggable')).toBe('false');
    expect(entry(palette, /^bm25/).getAttribute('draggable')).toBe('true');
  });
});

describe('the palette from the keyboard', () => {
  const entries = (palette: HTMLElement) => within(palette).getAllByRole('button');
  const stops = (palette: HTMLElement) => entries(palette).filter((b) => b.getAttribute('tabindex') !== '-1');

  it('is one tab stop, on its first entry until another takes focus', () => {
    const { palette } = renderPalette();
    expect(stops(palette)).toEqual([entries(palette)[0]]);
    act(() => entries(palette)[2]!.focus());
    expect(stops(palette)).toEqual([entries(palette)[2]]);
  });

  it('moves between entries with the arrow keys, Home and End, refused ones included, stopping at either end', () => {
    const { palette } = renderPalette();
    const all = entries(palette);
    act(() => all[0]!.focus());
    fireEvent.keyDown(all[0]!, { key: 'ArrowUp' });
    expect(document.activeElement).toBe(all[0]);
    fireEvent.keyDown(all[0]!, { key: 'ArrowDown' });
    expect(document.activeElement).toBe(all[1]);
    fireEvent.keyDown(all[1]!, { key: 'End' });
    expect(document.activeElement).toBe(all.at(-1));
    expect(all.at(-1)!.getAttribute('aria-disabled')).toBe('true');
    fireEvent.keyDown(all.at(-1)!, { key: 'ArrowDown' });
    expect(document.activeElement).toBe(all.at(-1));
    fireEvent.keyDown(all.at(-1)!, { key: 'Home' });
    expect(document.activeElement).toBe(all[0]);
    expect(stops(palette)).toEqual([all[0]]);
  });
});
