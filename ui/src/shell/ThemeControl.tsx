import { useLayoutEffect, useState } from 'react';
import { SegmentedControl } from '../../design/index.ts';
import { readStored, writeStored } from './storage.ts';

export type ThemeChoice = 'system' | 'light' | 'dark';

/** Where the viewer's choice is remembered, in this browser only. */
export const THEME_KEY = 'ragondin.theme';

const OPTIONS = [
  { value: 'system', label: 'System' },
  { value: 'light', label: 'Light' },
  { value: 'dark', label: 'Dark' },
] as const;

const isChoice = (value: string | null): value is ThemeChoice => OPTIONS.some((o) => o.value === value);

/**
 * The theme switch: the system's theme by default, or light or dark,
 * remembered per viewer. A choice sets `data-theme` on the page's root, which
 * design/tokens.css reads; the system's choice removes it, so the
 * `prefers-color-scheme` block applies (ARCHITECTURE.md § The design system).
 */
export function ThemeControl() {
  const [choice, setChoice] = useState<ThemeChoice>(() => {
    const stored = readStored('local', THEME_KEY);
    return isChoice(stored) ? stored : 'system';
  });

  useLayoutEffect(() => {
    const root = document.documentElement;
    if (choice === 'system') root.removeAttribute('data-theme');
    else root.setAttribute('data-theme', choice);
  }, [choice]);

  return (
    <SegmentedControl
      label="Theme"
      options={OPTIONS}
      value={choice}
      onChange={(value) => {
        if (!isChoice(value)) return;
        setChoice(value);
        // Not remembered when storage is unavailable; the page is right either way.
        writeStored('local', THEME_KEY, value);
      }}
    />
  );
}
