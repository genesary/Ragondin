import type { ReactNode } from 'react';
import { StatusDot } from '../StatusDot/StatusDot.tsx';
import './TopBar.css';

export type TopBarLink = { label: string; href: string; current?: boolean };
export type TopBarService = { name: string; connected: boolean };

export type TopBarProps = {
  /** The workspace, set in mono: its path, or the application's indicator of it. */
  workspace: ReactNode;
  /** The screens, in the order the application gives. */
  links: readonly TopBarLink[];
  services: readonly TopBarService[];
  /** The application's own status, such as its event stream, announced when it changes. */
  status?: { label: string; connected: boolean };
  /** The theme switch. */
  end?: ReactNode;
};

/**
 * The one persistent bar's shell: the name set in type (there is no logo
 * mark), the workspace, the screens as real links, the application's status
 * and each connected service. It never holds a screen's primary action.
 */
export function TopBar({ workspace, links, services, status, end }: TopBarProps) {
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
        {status === undefined ? null : (
          <span className="rg-service" data-connected={status.connected} role="status">
            <StatusDot connected={status.connected} />
            {status.label}
          </span>
        )}
        {services.map((s) => (
          <span key={s.name} className="rg-service" data-connected={s.connected}>
            <StatusDot connected={s.connected} />
            {s.connected ? s.name : `${s.name} unreachable`}
          </span>
        ))}
        {end}
      </div>
    </header>
  );
}
