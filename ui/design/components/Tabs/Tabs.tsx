import { useId, useRef, type KeyboardEvent } from 'react';
import { arrowStep, nextEnabled, tabStop } from '../../roving.ts';
import './Tabs.css';

export type Tab = { id: string; label: string; count?: number };

export type TabsProps = {
  label: string;
  tabs: readonly Tab[];
  selected: string;
  onSelect: (id: string) => void;
};

/**
 * Underline tabs between content sections of one screen. The panel is the
 * screen's: this is the tab list, with one tab stop and arrow-key movement.
 */
export function Tabs({ label, tabs, selected, onSelect }: TabsProps) {
  const refs = useRef<(HTMLButtonElement | null)[]>([]);
  const base = useId();
  const enabled = tabs.map(() => false);
  const stop = tabStop(enabled, tabs.findIndex((t) => t.id === selected));
  const onKeyDown = (e: KeyboardEvent) => {
    const step = arrowStep(e.key);
    if (step === null) return;
    // The arrows move the selection, not the page.
    e.preventDefault();
    const next = nextEnabled(enabled, stop, step);
    const tab = tabs[next];
    if (tab === undefined) return;
    onSelect(tab.id);
    refs.current[next]?.focus();
  };
  return (
    <div className="rg-tabs" role="tablist" aria-label={label}>
      {tabs.map((t, i) => (
        <button
          key={t.id}
          ref={(el) => {
            refs.current[i] = el;
          }}
          type="button"
          role="tab"
          id={`${base}-${t.id}`}
          aria-selected={t.id === selected}
          tabIndex={i === stop ? 0 : -1}
          onClick={() => onSelect(t.id)}
          onKeyDown={onKeyDown}
        >
          {t.label}
          {t.count === undefined ? null : <span className="rg-count">{t.count.toLocaleString('en-US')}</span>}
        </button>
      ))}
    </div>
  );
}
