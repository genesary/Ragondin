import type { ReactNode } from 'react';
import './TopBar.css';

export type TopBarLink = { label: string; href: string; current?: boolean };
export type TopBarService = { name: string; connected: boolean };

export type TopBarProps = {
  /** The workspace path, set in mono. */
  workspace: string;
  /** The screens, in the order the application gives. */
  links: readonly TopBarLink[];
  services: readonly TopBarService[];
  /** The theme switch. */
  end?: ReactNode;
};

/**
 * The one persistent bar's shell: the name set in type (there is no logo
 * mark), the workspace, the screens as real links, and each connected
 * service. It never holds a screen's primary action.
 */
export function TopBar({ workspace, links, services, end }: TopBarProps) {
  return (
    <header className="rg-topbar">
      <span className="rg-wordmark">Ragondin</span>
      <span className="rg-crumb">{workspace}</span>
      <nav className="rg-nav" aria-label="Screens">
        {links.map((l) => (
          <a key={l.href} href={l.href} aria-current={l.current ? 'page' : undefined}>
            {l.label}
          </a>
        ))}
      </nav>
      <div className="rg-topbar__end">
        {services.map((s) => (
          <span key={s.name} className="rg-service">
            <span className={s.connected ? 'rg-dot' : 'rg-dot is-off'} aria-hidden="true" />
            {s.connected ? s.name : `${s.name} unreachable`}
          </span>
        ))}
        {end}
      </div>
    </header>
  );
}
