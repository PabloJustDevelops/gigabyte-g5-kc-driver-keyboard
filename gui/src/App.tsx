import { useEffect, useState } from "react";
import { cn } from "./lib/cn";
import { rgbToHex, type Rgb, textOn } from "./lib/color";
import { useBacklight } from "./hooks/useBacklight";
import { Sidebar, type ViewId } from "./components/Sidebar";
import { TopChips } from "./components/TopChips";
import { HomeView } from "./views/HomeView";
import { LightingView } from "./views/LightingView";
import { ProfilesView } from "./views/ProfilesView";
import { PerformanceView } from "./views/PerformanceView";

const TITLES: Record<ViewId, [string, string]> = {
  home: ["Overview", "keyboard state at a glance"],
  lighting: ["Lighting", "colour · brightness · effects"],
  profiles: ["Profiles", "saved colour sets"],
  performance: ["Performance", "power & fan profiles (planned)"],
};

export default function App() {
  const kb = useBacklight();
  const { dev } = kb;
  const [view, setView] = useState<ViewId>("home");

  // live theme: CSS vars drive every accent on the page from the LED colour
  useEffect(() => {
    const root = document.documentElement;
    const rgb: Rgb = { r: dev.red, g: dev.green, b: dev.blue };
    const col = `rgb(${rgb.r} ${rgb.g} ${rgb.b})`;
    const on = dev.enabled;
    const pct = dev.brightness / 100;
    root.style.setProperty("--glowcol", col);
    root.style.setProperty("--glowp", String(on ? pct : 0));
    const accent = on ? col : "#3a414c";
    root.style.setProperty("--accent", accent);
    root.style.setProperty("--ring", accent);
    root.style.setProperty("--primary", accent);
    root.style.setProperty("--primary-foreground", on ? textOn(rgb) : "#f2f4f7");
  }, [dev.enabled, dev.red, dev.green, dev.blue, dev.brightness]);

  const lit = dev.enabled && dev.brightness > 0;
  const locked = dev.effectRunning;
  const fx = locked ? kb.mode : null;
  const rgb: Rgb = { r: dev.red, g: dev.green, b: dev.blue };
  const hex = rgbToHex(rgb);
  const [title, subtitle] = TITLES[view];

  const appClass = cn(
    "app",
    !dev.enabled && "off",
    lit && "lit",
    locked && "locked",
    fx === "breathe" && "fx-breathe",
    fx === "cycle" && "fx-cycle",
  );

  return (
    <div className={appClass}>
      <Sidebar
        active={view}
        effectRunning={dev.effectRunning}
        onNavigate={setView}
      />
      <main className="main">
        <header className="topbar">
          <div>
            <h1>{title}</h1>
            <div className="sub">{subtitle}</div>
          </div>
          <TopChips dev={dev} hex={hex} rgb={rgb} />
        </header>
        <div className="page">
          <div className={cn("page-inner", (view === "lighting" || view === "home") && "wide")}>
            {view === "home" && <HomeView kb={kb} hex={hex} go={setView} />}
            {view === "lighting" && <LightingView kb={kb} />}
            {view === "profiles" && <ProfilesView kb={kb} />}
            {view === "performance" && <PerformanceView />}
          </div>
        </div>
        <footer className={cn("status", kb.status.kind)}>{kb.status.msg}</footer>
      </main>
    </div>
  );
}
