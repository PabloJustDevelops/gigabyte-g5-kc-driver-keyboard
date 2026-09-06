import { ArrowRight } from "lucide-react";
import { Button } from "../components/ui/button";
import { KeyboardPreview } from "../components/KeyboardPreview";
import { Section } from "../components/Section";
import { cn } from "../lib/cn";
import type { Backlight } from "../hooks/useBacklight";
import type { ViewId } from "../components/Sidebar";

interface Props {
  kb: Backlight;
  hex: string;
  go: (v: ViewId) => void;
}

export function HomeView({ kb, hex, go }: Props) {
  const { dev, mode } = kb;
  const effect = dev.effectRunning
    ? mode
      ? `${mode} @ ${kb.speed}`
      : "running"
    : "idle";
  const lit = dev.enabled && dev.brightness > 0;

  return (
    /* the object opens the page; instrumentation sits beside it */
    <div className="home-grid">
      <KeyboardPreview lit={lit} running={dev.effectRunning} fx={dev.effectRunning ? mode : null} hero />

      <div className="home-side">
        <div className="big-readout home-readout" role="img" aria-label={`Current colour #${hex}`}>
          <span className="big-hex" style={{ color: lit ? `#${hex}` : undefined }}>
            #{hex}
          </span>
          <span className="big-rgb mono-dim">
            {dev.red} · {dev.green} · {dev.blue}
            {dev.enabled ? ` · ${dev.brightness}%` : " · off"}
          </span>
        </div>

        <Section title="Status" readout={lit ? "LIVE" : dev.enabled ? "DIM" : "OFF"}>
          <div className="kv">
            <span className="kv-k">Effect</span>
            <span className={cn("kv-v", dev.effectRunning ? "running" : "dim")}>
              {effect}
              {dev.effectRunning && " · controls locked"}
            </span>
          </div>
          <div className="kv">
            <span className="kv-k">Backend</span>
            <span className="kv-v">
              <span className="chip">
                <span className={cn("dot", dev.backend.startsWith("led") ? "live" : "ec")} />
                {dev.backend}
              </span>
            </span>
          </div>
          <div className="readout" style={{ marginTop: 10 }}>
            saved state is restored at boot and after suspend
          </div>
          <div className="overview-go">
            <Button onClick={() => go("lighting")}>
              Open Lighting
              <ArrowRight />
            </Button>
          </div>
        </Section>
      </div>
    </div>
  );
}
