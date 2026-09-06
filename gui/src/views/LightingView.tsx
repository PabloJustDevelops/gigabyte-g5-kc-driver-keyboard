import { KeyboardPreview } from "../components/KeyboardPreview";
import { ColorPanel } from "../components/ColorPanel";
import { BrightnessPanel } from "../components/BrightnessPanel";
import { EffectsPanel } from "../components/EffectsPanel";
import type { Backlight } from "../hooks/useBacklight";

interface Props {
  kb: Backlight;
}

export function LightingView({ kb }: Props) {
  const { dev, mode } = kb;
  const lit = dev.enabled && dev.brightness > 0;
  const locked = dev.effectRunning;
  const fx = locked ? mode : null;
  const hex = [dev.red, dev.green, dev.blue]
    .map((v) => v.toString(16).padStart(2, "0"))
    .join("");

  return (
    <div className="lighting-grid">
      {/* focal column: the object + its reading */}
      <div className="lighting-lead">
        <KeyboardPreview lit={lit} running={locked} fx={fx} hero />
        <div className="big-readout" role="img" aria-label={`Current colour #${hex}`}>
          <span className="big-hex" style={{ color: lit ? `#${hex}` : undefined }}>
            #{hex}
          </span>
          <span className="big-rgb mono-dim">
            {dev.red} · {dev.green} · {dev.blue}
            {dev.enabled ? "" : " · off"}
          </span>
        </div>
      </div>

      {/* instrument column: dominant choice first, rest secondary */}
      <div className="lighting-side">
        <ColorPanel
          dev={dev}
          locked={locked}
          setDragging={kb.setDragging}
          previewColor={kb.previewColor}
          commitColor={kb.commitColor}
        />
        <BrightnessPanel
          dev={dev}
          locked={locked}
          setDragging={kb.setDragging}
          liveBrightness={kb.liveBrightness}
          commitBrightness={kb.commitBrightness}
          togglePower={kb.togglePower}
        />
        <EffectsPanel
          dev={dev}
          mode={mode}
          speed={kb.speed}
          pickMode={kb.pickMode}
          changeSpeed={kb.changeSpeed}
          restartEffectAtSpeed={kb.restartEffectAtSpeed}
          startEffect={kb.startEffect}
          stopEffect={kb.stopEffect}
        />
      </div>
    </div>
  );
}
