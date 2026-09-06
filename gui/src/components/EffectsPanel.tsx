import { Button } from "./ui/button";
import { Slider } from "./ui/slider";
import { Section } from "./Section";
import { cn } from "../lib/cn";
import type { DeviceState, EffectMode } from "../lib/types";

interface Props {
  dev: DeviceState;
  mode: EffectMode | null;
  speed: number;
  pickMode: (m: EffectMode) => void;
  changeSpeed: (n: number) => void;
  restartEffectAtSpeed: (n: number) => void;
  startEffect: () => void;
  stopEffect: () => void;
}

export function EffectsPanel({
  dev,
  mode,
  speed,
  pickMode,
  changeSpeed,
  restartEffectAtSpeed,
  startEffect,
  stopEffect,
}: Props) {
  const running = dev.effectRunning;
  const modes: Array<{ id: EffectMode; name: string; desc: string }> = [
    { id: "breathe", name: "Breathe", desc: "pulsing glow" },
    { id: "cycle", name: "Cycle", desc: "rainbow sweep" },
  ];

  return (
    <Section title="Effects" readout="host-driven · stops on reboot">
      <div className="modes" role="group" aria-label="Effect mode">
        {modes.map((m) => (
          <button
            key={m.id}
            type="button"
            className={cn("mode", mode === m.id && "active")}
            aria-pressed={mode === m.id}
            onClick={() => pickMode(m.id)}
          >
            <span className="mode-name">{m.name}</span>
            <span className="mode-desc">{m.desc}</span>
          </button>
        ))}
      </div>

      <div className="speed-row">
        <span className="speed-label">speed</span>
        <Slider
          value={speed}
          min={1}
          max={10}
          step={1}
          ariaLabel="Effect speed"
          onValueChange={(v) => changeSpeed(v)}
          onValueCommit={(v) => restartEffectAtSpeed(v)}
        />
        <span className="speed-val">{speed}</span>
      </div>

      <div className="action-row">
        <Button
          className="flex-1"
          disabled={!mode || running}
          onClick={startEffect}
        >
          {mode ? `run ${mode} @ ${speed}` : "pick a mode"}
        </Button>
        <Button variant="destructive" disabled={!running} onClick={stopEffect}>
          stop
        </Button>
      </div>
    </Section>
  );
}
