/* Backend bridge.
 *
 * Talks to the Rust core exclusively through invoke() (Tauri commands that
 * wrap the g5kbd CLI). In a plain browser (no Tauri host) it falls back to an
 * in-memory mock so the panel can be previewed on http://localhost:5173.
 */

import { invoke } from "@tauri-apps/api/core";
import type { DeviceState, EffectMode, Profile } from "./types";
import { hexToRgb, rgbToHex, type Rgb } from "./color";

export const IS_TAURI = "__TAURI_INTERNALS__" in window;

interface ServerState {
  enabled: boolean;
  red: number;
  green: number;
  blue: number;
  brightness: number;
  effect_running: boolean;
  backend: string;
}

const toDevice = (s: ServerState): DeviceState => ({
  enabled: s.enabled,
  red: s.red,
  green: s.green,
  blue: s.blue,
  brightness: s.brightness,
  effectRunning: s.effect_running,
  backend: s.backend,
});

const clamp = (n: number, lo: number, hi: number) => Math.min(hi, Math.max(lo, n));

/* ── typed wrappers ─────────────────────────────────────── */

export async function getState(): Promise<DeviceState> {
  if (!IS_TAURI) return mockGetState();
  return toDevice(await invoke<ServerState>("get_state"));
}

export async function getBackend(): Promise<string> {
  if (!IS_TAURI) return mock.backend;
  return invoke<string>("backend");
}

export async function setColor(rgb: Rgb): Promise<string> {
  if (!IS_TAURI) return mockSetColor(rgb);
  return invoke<string>("set_color", { red: rgb.r, green: rgb.g, blue: rgb.b });
}

export async function setBrightnessPct(pct: number): Promise<string> {
  if (!IS_TAURI) return mockSetBrightness(pct);
  return invoke<string>("set_brightness", { pct: clamp(Math.round(pct), 0, 100) });
}

/** Live write used while dragging; only meaningful with the kernel LED node. */
export async function brightnessRaw(level: number): Promise<void> {
  if (!IS_TAURI) return mockBrightnessRaw(level);
  return invoke<void>("brightness_raw", { level: clamp(Math.round(level), 0, 255) });
}

export async function setPower(on: boolean): Promise<string> {
  if (!IS_TAURI) return mockSetPower(on);
  return invoke<string>("set_power", { on });
}

export async function effectStart(mode: EffectMode, speed: number): Promise<string> {
  if (!IS_TAURI) return mockEffectStart(mode, speed);
  return invoke<string>("effect_start", { mode, speed: clamp(Math.round(speed), 1, 10) });
}

export async function effectStop(): Promise<string> {
  if (!IS_TAURI) return mockEffectStop();
  return invoke<string>("effect_stop");
}

export async function profilesList(): Promise<Profile[]> {
  const raw = IS_TAURI
    ? await invoke<string[]>("profiles_list")
    : mockProfiles.map((p) => p.join("\t"));
  const out: Profile[] = [];
  for (const line of raw) {
    const [name, color, bri, state] = line.split("\t");
    if (!name) continue;
    out.push({ name, color, brightness: Number(bri), state });
  }
  return out;
}

export async function profileSave(name: string): Promise<string> {
  if (!IS_TAURI) return mockProfileSave(name);
  return invoke<string>("profile_save", { name });
}

export async function profileApply(name: string): Promise<string> {
  if (!IS_TAURI) return mockProfileApply(name);
  return invoke<string>("profile_apply", { name });
}

export async function profileDelete(name: string): Promise<string> {
  if (!IS_TAURI) return mockProfileDelete(name);
  return invoke<string>("profile_delete", { name });
}

/* ── browser mock (mirrors the CLI semantics) ───────────── */

const delay = () => new Promise((r) => setTimeout(r, 30));

interface MockState extends DeviceState {
  effectMode: EffectMode | null;
}

const mock: MockState = {
  enabled: true,
  red: 0,
  green: 0,
  blue: 200,
  brightness: 100,
  effectRunning: false,
  backend: "led (mock)",
  effectMode: null,
};

let mockProfiles: string[][] = [["gaming", "00aaff", "100", "on"]];

const mockGetState = async (): Promise<DeviceState> => {
  await delay();
  return {
    enabled: mock.enabled,
    red: mock.red,
    green: mock.green,
    blue: mock.blue,
    brightness: mock.brightness,
    effectRunning: mock.effectRunning,
    backend: "led (mock)",
  };
};

const mockSetColor = async (rgb: Rgb): Promise<string> => {
  await delay();
  mock.red = rgb.r;
  mock.green = rgb.g;
  mock.blue = rgb.b;
  return `keyboard backlight: #${rgbToHex(rgb)}`;
};

const mockSetBrightness = async (pct: number): Promise<string> => {
  await delay();
  mock.brightness = pct;
  return `brightness ${pct}%`;
};

const mockBrightnessRaw = async (level: number): Promise<void> => {
  await delay();
  mock.brightness = Math.round((level / 255) * 100);
};

const mockSetPower = async (on: boolean): Promise<string> => {
  await delay();
  mock.enabled = on;
  return on ? "on" : "off";
};

const mockEffectStart = async (mode: EffectMode, speed: number): Promise<string> => {
  await delay();
  mock.effectRunning = true;
  mock.effectMode = mode;
  return `effect ${mode} running (speed ${speed})`;
};

const mockEffectStop = async (): Promise<string> => {
  await delay();
  mock.effectRunning = false;
  mock.effectMode = null;
  return "effect stopped";
};

const mockProfileSave = async (name: string): Promise<string> => {
  await delay();
  mockProfiles = mockProfiles.filter((p) => p[0] !== name);
  mockProfiles.push([
    name,
    rgbToHex({ r: mock.red, g: mock.green, b: mock.blue }),
    String(mock.brightness),
    "on",
  ]);
  return `profile saved: ${name}`;
};

const mockProfileApply = async (name: string): Promise<string> => {
  await delay();
  const p = mockProfiles.find((x) => x[0] === name);
  if (!p) throw new Error(`unknown profile: ${name}`);
  const rgb = hexToRgb(p[1]);
  if (rgb) {
    mock.red = rgb.r;
    mock.green = rgb.g;
    mock.blue = rgb.b;
  }
  mock.brightness = Number(p[2]);
  return `applied profile ${name}`;
};

const mockProfileDelete = async (name: string): Promise<string> => {
  await delay();
  mockProfiles = mockProfiles.filter((p) => p[0] !== name);
  return `profile deleted: ${name}`;
};
