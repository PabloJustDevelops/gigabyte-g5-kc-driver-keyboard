import { useEffect, useRef, useState, type CSSProperties } from "react";
import { Button } from "./ui/button";
import { Section } from "./Section";
import { cn } from "../lib/cn";
import { hexToRgb, rgbToHex, type Rgb } from "../lib/color";
import type { DeviceState } from "../lib/types";

const PRESETS: Array<[string, string]> = [
  ["ff0033", "Crimson"],
  ["ff6a00", "Orange"],
  ["ffd000", "Amber"],
  ["00e676", "Green"],
  ["00d5ff", "Cyan"],
  ["2f7bff", "Blue"],
  ["7c4dff", "Violet"],
  ["ff2e88", "Pink"],
  ["ffffff", "White"],
];

interface Props {
  dev: DeviceState;
  locked: boolean;
  setDragging: (v: boolean) => void;
  previewColor: (rgb: Rgb) => void;
  commitColor: (rgb: Rgb) => void;
}

export function ColorPanel({
  dev,
  locked,
  setDragging,
  previewColor,
  commitColor,
}: Props) {
  const rgb: Rgb = { r: dev.red, g: dev.green, b: dev.blue };
  const hex = rgbToHex(rgb);

  /* native picker — debounced commit, don't fight an open dialog */
  const nativeRef = useRef<HTMLInputElement>(null);
  const picking = useRef(false);
  const commitTimer = useRef<number | undefined>(undefined);

  const onNativeInput = () => {
    const el = nativeRef.current;
    if (!el) return;
    const parsed = hexToRgb(el.value.slice(1));
    if (!parsed) return;
    previewColor(parsed);
    if (commitTimer.current !== undefined) window.clearTimeout(commitTimer.current);
    commitTimer.current = window.setTimeout(() => commitColor(parsed), 160);
  };

  useEffect(() => {
    const el = nativeRef.current;
    if (!el) return;
    const onInput = () => onNativeInput();
    const onFocus = () => {
      picking.current = true;
    };
    const onBlur = () => {
      window.setTimeout(() => {
        picking.current = false;
      }, 250);
      if (commitTimer.current !== undefined) window.clearTimeout(commitTimer.current);
    };
    el.addEventListener("input", onInput);
    el.addEventListener("focus", onFocus);
    el.addEventListener("blur", onBlur);
    return () => {
      el.removeEventListener("input", onInput);
      el.removeEventListener("focus", onFocus);
      el.removeEventListener("blur", onBlur);
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // sync the picker swatch from outside (profile apply / poll) unless open
  useEffect(() => {
    const el = nativeRef.current;
    if (el && !picking.current) el.value = "#" + hex;
  }, [hex]);

  /* hex entry */
  const [hexDraft, setHexDraft] = useState(hex);
  const [bad, setBad] = useState(false);
  const hexFocus = useRef(false);
  useEffect(() => {
    if (!hexFocus.current) setHexDraft(hex);
  }, [hex]);

  const applyHexDraft = () => {
    const parsed = hexToRgb(hexDraft);
    if (parsed) {
      setBad(false);
      commitColor(parsed);
    } else {
      setBad(true);
    }
  };

  /* RGB fine-tune commit-on-release (window-level, so releasing
     outside the input still commits) */
  const fineRgb = (r: number, g: number, b: number): Rgb => ({
    r: Math.max(0, Math.min(255, r)),
    g: Math.max(0, Math.min(255, g)),
    b: Math.max(0, Math.min(255, b)),
  });
  const [dragActive, setDragActive] = useState(false);
  const rgbRef = useRef(rgb);
  rgbRef.current = rgb;
  const commitRef = useRef(commitColor);
  commitRef.current = commitColor;
  useEffect(() => {
    if (!dragActive) return;
    const up = () => {
      setDragActive(false);
      setDragging(false);
      const cur = rgbRef.current;
      commitRef.current(fineRgb(cur.r, cur.g, cur.b));
    };
    const cancel = () => {
      setDragActive(false);
      setDragging(false);
    };
    window.addEventListener("pointerup", up);
    window.addEventListener("pointercancel", cancel);
    return () => {
      window.removeEventListener("pointerup", up);
      window.removeEventListener("pointercancel", cancel);
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [dragActive]);

  return (
    <Section title="Colour" readout={`${rgb.r} · ${rgb.g} · ${rgb.b}`} className="dominant">
      <div className="swatches" role="group" aria-label="Colour presets">
        {PRESETS.map(([col, name]) => (
          <button
            key={col}
            type="button"
            className={cn("swatch", hex === col && "sel")}
            style={{ "--sw": "#" + col } as CSSProperties}
            title={name}
            aria-label={name}
            disabled={locked}
            onClick={() => commitColor(hexToRgb(col)!)}
          />
        ))}
      </div>

      <div className="picker-row">
        <label title="Open colour picker">
          <input
            ref={nativeRef}
            type="color"
            defaultValue={"#" + hex}
            disabled={locked}
            aria-label="Open colour picker"
          />
        </label>
        <span className="hex-hash">#</span>
        <input
          className={cn("hex-input", bad && "bad")}
          maxLength={6}
          spellCheck={false}
          aria-label="Hex colour"
          value={hexDraft}
          disabled={locked}
          onFocus={() => {
            hexFocus.current = true;
            setBad(false);
          }}
          onBlur={() => {
            hexFocus.current = false;
          }}
          onChange={(e) => {
            setHexDraft(e.target.value.toLowerCase());
            const parsed = hexToRgb(e.target.value);
            if (parsed) {
              setBad(false);
              previewColor(parsed);
            } else if (e.target.value.length === 6) setBad(true);
          }}
          onKeyDown={(e) => {
            if (e.key === "Enter") applyHexDraft();
          }}
        />
        <Button variant="ghost" size="sm" disabled={locked} onClick={applyHexDraft}>
          set
        </Button>
      </div>

      <details className="finerow">
        <summary>RGB fine-tune</summary>
        <div className="rgb">
          {(["r", "g", "b"] as const).map((ch) => {
            const chCls = ch === "r" ? "red" : ch === "g" ? "green" : "blue";
            return (
              <div className="rgb-row" key={ch}>
                <span className="rgb-label">
                  <span className={cn("ch", chCls)}>{ch.toUpperCase()}</span>
                  <span>{rgb[ch]}</span>
                </span>
                <input
                  type="range"
                  min={0}
                  max={255}
                  step={1}
                  className={cn("rgb-slider", `${chCls}-slider`)}
                  value={rgb[ch]}
                  disabled={locked}
                  onPointerDown={() => {
                    setDragActive(true);
                    setDragging(true);
                  }}
                  onBlur={() => setDragging(false)}
                  onChange={(e) => {
                    const v = Number(e.target.value);
                    const next =
                      ch === "r"
                        ? fineRgb(v, rgb.g, rgb.b)
                        : ch === "g"
                          ? fineRgb(rgb.r, v, rgb.b)
                          : fineRgb(rgb.r, rgb.g, v);
                    previewColor(next);
                  }}
                />
              </div>
            );
          })}
        </div>
      </details>
    </Section>
  );
}
