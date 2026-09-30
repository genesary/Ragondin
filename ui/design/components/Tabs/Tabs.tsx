import { useRef } from 'react';
import { arrowStep, nextEnabled } from '../../roving.ts';
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
  const current = tabs.findIndex((t) => t.id === selected);
  const onKeyDown = (key: string) => {
    const step = arrowStep(key);
    if (step === null) return;
    const next = nextEnabled(tabs.map(() => false), current, step);
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
          id={`tab-${t.id}`}
          aria-selected={t.id === selected}
          tabIndex={t.id === selected ? 0 : -1}
          onClick={() => onSelect(t.id)}
          onKeyDown={(e) => onKeyDown(e.key)}
        >
          {t.label}
          {t.count === undefined ? null : <span className="rg-count">{t.count.toLocaleString('en-US')}</span>}
        </button>
      ))}
    </div>
  );
}
