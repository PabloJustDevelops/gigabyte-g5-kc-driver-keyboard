import { cn } from "../lib/cn";
import type { Rgb } from "../lib/color";
import type { DeviceState } from "../lib/types";

interface Props {
  dev: DeviceState;
  hex: string;
  rgb: Rgb;
}

export function TopChips({ dev, hex, rgb }: Props) {
  void rgb;
  const backend = dev.backend.startsWith("led") ? "live" : "ec";
  return (
    <div className="head-right">
      <div
        className="live-chip"
        title={`Current colour on the keyboard · rgb(${rgb.r} ${rgb.g} ${rgb.b})`}
      >
        <span className="dot live-dot" />
        <span className="hex-chip">#{hex}</span>
      </div>
      <div className="pill" title="Backend in use">
        <span className={cn("dot", backend)} />
        <span>{dev.backend}</span>
      </div>
    </div>
  );
}
