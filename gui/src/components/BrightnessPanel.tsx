import { Slider } from "./ui/slider";
import { Switch } from "./ui/switch";
import { Section } from "./Section";
import { cn } from "../lib/cn";
import type { DeviceState } from "../lib/types";

interface Props {
  dev: DeviceState;
  locked: boolean;
  setDragging: (v: boolean) => void;
  liveBrightness: (pct: number) => void;
  commitBrightness: (pct: number) => void;
  togglePower: () => void;
}

export function BrightnessPanel({
  dev,
  locked,
  setDragging,
  liveBrightness,
  commitBrightness,
  togglePower,
}: Props) {
  return (
    <Section title="Brightness" readout={`${dev.brightness}%`}>
      <div className="bright-row">
        <div
          className="flex-1"
          onPointerDown={() => setDragging(true)}
          onPointerUp={() => setDragging(false)}
          onPointerCancel={() => setDragging(false)}
        >
          <Slider
            value={dev.brightness}
            min={0}
            max={100}
            disabled={locked}
            ariaLabel="Brightness"
            onValueChange={(v) => liveBrightness(v)}
            onValueCommit={(v) => commitBrightness(v)}
          />
        </div>
        <div className="flex items-center gap-2">
          <span
            className={cn(
              "font-mono text-[11px] tracking-wide",
              dev.enabled ? "text-foreground" : "text-muted-foreground",
            )}
          >
            {dev.enabled ? "on" : "off"}
          </span>
          <Switch
            checked={dev.enabled}
            onCheckedChange={() => togglePower()}
            disabled={locked}
            ariaLabel="Backlight power"
          />
        </div>
      </div>
    </Section>
  );
}
