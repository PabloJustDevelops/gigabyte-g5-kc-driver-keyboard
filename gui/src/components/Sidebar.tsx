import { Gauge, House, Keyboard, Palette, type LucideIcon } from "lucide-react";
import { cn } from "../lib/cn";

export type ViewId = "home" | "lighting" | "profiles" | "performance";

interface Item {
  id: ViewId;
  label: string;
  icon: LucideIcon;
  dot?: boolean;
  badge?: string;
}

interface Props {
  active: ViewId;
  effectRunning: boolean;
  onNavigate: (v: ViewId) => void;
}

export function Sidebar({ active, effectRunning, onNavigate }: Props) {
  const items: Item[] = [
    { id: "home", label: "Overview", icon: House },
    { id: "lighting", label: "Lighting", icon: Keyboard, dot: effectRunning },
    { id: "profiles", label: "Profiles", icon: Palette },
  ];
  const system: Item[] = [
    { id: "performance", label: "Performance", icon: Gauge, badge: "planned" },
  ];

  const renderItem = (it: Item) => {
    const Icon = it.icon;
    return (
      <button
        key={it.id}
        type="button"
        className={cn("nav-item", active === it.id && "active")}
        aria-current={active === it.id ? "page" : undefined}
        aria-label={it.dot ? `${it.label} — effect running` : undefined}
        onClick={() => onNavigate(it.id)}
      >
        <Icon className="nav-ico" strokeWidth={1.8} />
        <span>{it.label}</span>
        {it.dot && (
          <span className="nav-dot" title="effect running" aria-hidden="true" />
        )}
        {it.badge && <span className="nav-badge">{it.badge}</span>}
      </button>
    );
  };

  return (
    <aside className="side">
      <div className="side-brand">
        <div className="brand-mark" aria-hidden="true">
          <i />
          <i />
          <i />
        </div>
        <div className="brand-text">
          <h1>g5kbd</h1>
          <p>Gigabyte G5 · control center</p>
        </div>
      </div>

      <nav className="nav" aria-label="Sections">
        <div className="nav-group">Backlight</div>
        {items.map(renderItem)}
        <div className="nav-group">System</div>
        {system.map(renderItem)}
      </nav>

      <div className="side-foot">
        <div>
          g5kbd <b>v0.3.0</b>
        </div>
        <div>kernel driver · CLI · GUI</div>
      </div>
    </aside>
  );
}
