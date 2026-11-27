import { Activity, Database, LayoutGrid, Network, Palette } from 'lucide-react';
import type { ReactNode } from 'react';
import { NavLink, Outlet } from 'react-router';

import { designSystemEnabled } from '@/app/flags.ts';
import { useMediaQuery } from '@/hooks/useMediaQuery.ts';
import { useNow } from '@/hooks/useNow.ts';
import { useSystemStatus } from '@/lib/api/queries.ts';
import { cn } from '@/lib/cn.ts';
import { formatUtcClock } from '@/lib/format/time.ts';

import { Separator } from '../ui/Panel.tsx';
import { Tooltip } from '../ui/Tooltip.tsx';
import { ConnectionIndicator } from './ConnectionIndicator.tsx';
import { ModeIndicator } from './ModeIndicator.tsx';
import { Wordmark } from './Wordmark.tsx';

interface NavItem {
  to: string;
  label: string;
  icon: ReactNode;
}

const navItems: NavItem[] = [
  { to: '/', label: 'Overview', icon: <LayoutGrid className="size-4" /> },
  { to: '/gateway', label: 'Gateway', icon: <Network className="size-4" /> },
  { to: '/data', label: 'Data engine', icon: <Database className="size-4" /> },
  { to: '/system', label: 'System', icon: <Activity className="size-4" /> },
  ...(designSystemEnabled
    ? [{ to: '/design-system', label: 'Design system', icon: <Palette className="size-4" /> }]
    : []),
];

function UtcClock() {
  const now = useNow(1_000);
  return (
    <span className="num text-xs text-fg-secondary" aria-label="Current time, UTC">
      {formatUtcClock(now)} <span className="text-fg-muted">UTC</span>
    </span>
  );
}

function EnvironmentTag() {
  const env = useSystemStatus().data?.data.environment;
  if (!env) return null;
  return (
    <span className="text-2xs font-medium tracking-widest text-fg-muted uppercase">{env}</span>
  );
}

/**
 * Global shell:
 *   status bar (44 px)  identity · operating mode · connectivity · clock
 *   primary navigation  rail (56 px) below 1200 px, labelled (200 px) above;
 *                       horizontal strip under 768 px
 *   workspace           routed page
 *
 * Mode and connectivity stay visible at every width.
 */
export function AppShell() {
  return (
    <div className="grid min-h-full grid-rows-[auto_auto_1fr] bg-canvas md:h-full md:grid-cols-[var(--nav-width-collapsed)_1fr] md:grid-rows-[var(--statusbar-height)_1fr] xl:grid-cols-[var(--nav-width)_1fr]">
      <a
        href="#workspace"
        className="sr-only z-[var(--z-tooltip)] rounded-md bg-fg px-3 py-2 text-canvas focus:not-sr-only focus:fixed focus:top-2 focus:left-2"
      >
        Skip to workspace
      </a>

      <header
        className="z-[var(--z-statusbar)] flex flex-wrap items-center gap-x-4 gap-y-2 border-b border-line bg-base px-3 py-2 md:col-span-2 md:flex-nowrap md:py-0"
        aria-label="Status bar"
      >
        <Wordmark />
        <Separator orientation="vertical" className="hidden h-5 md:block" />
        <div className="order-3 w-full md:order-none md:w-auto">
          <ModeIndicator />
        </div>
        <div className="ml-auto flex items-center gap-4">
          <span className="hidden lg:inline">
            <EnvironmentTag />
          </span>
          <ConnectionIndicator />
          <span className="hidden sm:inline">
            <UtcClock />
          </span>
        </div>
      </header>

      <nav
        aria-label="Primary"
        className="z-[var(--z-nav)] flex gap-1 overflow-x-auto border-b border-line bg-base px-2 py-1.5 md:flex-col md:overflow-visible md:border-r md:border-b-0 md:px-2 md:py-3"
      >
        {navItems.map((item) => (
          <NavItemLink key={item.to} item={item} />
        ))}
      </nav>

      <main id="workspace" tabIndex={-1} className="min-w-0 overflow-y-auto outline-none">
        <Outlet />
      </main>
    </div>
  );
}

function NavItemLink({ item }: { item: NavItem }) {
  const link = (
    <NavLink
      to={item.to}
      end={item.to === '/'}
      className={({ isActive }) =>
        cn(
          'flex h-8 shrink-0 items-center gap-2.5 rounded-md px-2.5 text-sm transition-ui md:h-9 md:justify-center md:px-0 xl:justify-start xl:px-2.5',
          isActive
            ? 'bg-selected text-fg shadow-[inset_2px_0_0_var(--color-fg)]'
            : 'text-fg-secondary hover:bg-hover hover:text-fg',
        )
      }
    >
      <span aria-hidden="true" className="flex">
        {item.icon}
      </span>
      {/* Label visible on mobile strip and wide rail; screen-reader-only on the icon rail. */}
      <span className="md:sr-only xl:not-sr-only">{item.label}</span>
    </NavLink>
  );
  // Icon rail (768-1199 px): the label is screen-reader-only, so offer it as a tooltip.
  const iconRail = useMediaQuery('(min-width: 768px) and (max-width: 1199.98px)');
  return iconRail ? (
    <Tooltip content={item.label} side="right">
      {link}
    </Tooltip>
  ) : (
    link
  );
}
