/* Backend bridge.
 *
 * Talks to the Rust core exclusively through invoke() (Tauri commands that
 * wrap the g5kbd CLI). In a plain browser (no Tauri host) it falls back to an
 * in-memory mock so the panel can be previewed on http://localhost:5173.
 */

import { invoke } from "@tauri-apps/api/core";
import type {
  CurvePoint,
  DeviceState,
  EffectMode,
  FanMode,
  FanStatus,
  Profile,
} from "./types";
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

/* ── fans ──────────────────────────────────────────────────── */

/** `g5fan status --json` as the Rust core hands it over (still snake_case). */
interface ServerFanStatus {
  mode: string;
  backend: string;
  driver: boolean;
  manual_duty: number | null;
  curve: [number, number][] | null;
  custom_curve: [number, number][];
  presets: Record<string, [number, number][]>;
  fans: {
    label: string;
    duty_pct: number | null;
    rpm: number | null;
    tacho: number | null;
  }[];
  cpu_temp_c: number | null;
  gpu_temp_c: number | null;
  ceiling_c: number;
  daemon: boolean;
  age_s: number;
  stale: boolean;
}

const toPoints = (pts: [number, number][]): CurvePoint[] =>
  pts.map(([t, d]) => [t, d] as CurvePoint);

const toFanStatus = (s: ServerFanStatus): FanStatus => ({
  mode: s.mode,
  backend: s.backend,
  driver: s.driver,
  manualDuty: s.manual_duty,
  curve: s.curve ? toPoints(s.curve) : null,
  customCurve: toPoints(s.custom_curve),
  presets: Object.fromEntries(
    Object.entries(s.presets).map(([name, pts]) => [name, toPoints(pts)]),
  ),
  fans: s.fans.map((f) => ({
    label: f.label,
    duty: f.duty_pct,
    rpm: f.rpm,
    tacho: f.tacho,
  })),
  cpuTempC: s.cpu_temp_c,
  gpuTempC: s.gpu_temp_c,
  ceilingC: s.ceiling_c,
  daemon: s.daemon,
  ageS: s.age_s,
  stale: s.stale,
});

/** What the fan daemon last saw, published to /run/g5fan every tick.
 *
 * Deliberately unprivileged: the panel must never raise a password dialog
 * just to draw a fan, and polling every few seconds means it could not ask
 * anyway. Only the writes below go through polkit. */
export async function fanStatus(): Promise<FanStatus> {
  if (!IS_TAURI) return mockFanStatus();
  return toFanStatus(await invoke<ServerFanStatus>("fan_status"));
}

export async function fanSetMode(mode: FanMode | `manual:${number}`): Promise<string> {
  if (!IS_TAURI) return mockFanSetMode(mode);
  return invoke<string>("fan_set_mode", { mode });
}

export async function fanSetManual(pct: number): Promise<string> {
  if (!IS_TAURI) return mockFanSetMode(`manual:${pct}`);
  return invoke<string>("fan_set_manual", { pct: clamp(Math.round(pct), 0, 100) });
}

/** Write the custom curve; the daemon turns it into duty for the fans. */
export async function fanSetCurve(points: CurvePoint[]): Promise<string> {
  if (!IS_TAURI) return mockFanSetCurve(points);
  return invoke<string>("fan_set_curve", { points });
}

export function formatCurve(points: CurvePoint[]): string {
  return points.map(([t, d]) => `${t}:${d}`).join(" ");
}

export async function fanWatchdog(): Promise<boolean> {
  if (!IS_TAURI) return mockFanWatchdog();
  return (await invoke<string>("fan_watchdog")) === "active";
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

/* ── fan mock ─────────────────────────────────────────────── */

/* Mirrors g5fan's own presets (PROFILES in src/g5fan.py) — in Tauri they
 * arrive in `presets`, so the panel never carries a second copy. */

const mockFan = {
  mode: "auto",
  manualDuty: null as number | null,
  customCurve: [[50, 25], [65, 40], [75, 60], [85, 80], [95, 100]] as CurvePoint[],
};

const MOCK_PRESETS: Record<string, CurvePoint[]> = {
  silent: [[45, 15], [60, 25], [72, 40], [82, 65], [92, 100]],
  maxq: [[48, 12], [62, 20], [75, 35], [85, 60], [92, 100]],
  custom: mockFan.customCurve,
};

/** Duty that matches the mode, so a click is visible on the gauges. */
function mockDutyFor(mode: string): number {
  if (mode === "turbo") return 100;
  if (mode === "manual") return mockFan.manualDuty ?? 50;
  if (mode === "silent") return 18;
  if (mode === "maxq") return 15;
  if (mode === "custom") return 40;
  return 35; // auto — the firmware curve at the mock temperatures
}

const mockRpm = (duty: number) => (duty <= 0 ? 0 : 600 + duty * 26);

let mockDaemon = true;

const mockFanStatus = async (): Promise<FanStatus> => {
  await delay();
  const cpuDuty = mockDutyFor(mockFan.mode);
  const gpuDuty = Math.max(0, cpuDuty - 5);
  return {
    mode: mockFan.mode,
    backend: "kernel (mock)",
    driver: true,
    manualDuty: mockFan.manualDuty,
    curve: mockFan.mode in MOCK_PRESETS ? MOCK_PRESETS[mockFan.mode] : null,
    customCurve: mockFan.customCurve.map((p) => [...p] as CurvePoint),
    presets: MOCK_PRESETS,
    fans: [
      { label: "CPU", duty: cpuDuty, rpm: mockRpm(cpuDuty), tacho: 972 },
      { label: "GPU", duty: gpuDuty, rpm: mockRpm(gpuDuty), tacho: 1020 },
    ],
    cpuTempC: 52,
    gpuTempC: 44,
    ceilingC: 100,
    daemon: mockDaemon,
    ageS: 1.2,
    stale: false,
  };
};

const mockFanSetMode = async (mode: string): Promise<string> => {
  await delay();
  mockFan.mode = mode.split(":")[0];
  mockFan.manualDuty = mode.startsWith("manual")
    ? parseInt(mode.split(":")[1] ?? "50", 10)
    : null;
  return `fans: ${mode}`;
};

const mockFanSetCurve = async (points: CurvePoint[]): Promise<string> => {
  await delay();
  mockFan.customCurve = points.map((p) => [...p] as CurvePoint);
  MOCK_PRESETS.custom = mockFan.customCurve;
  mockFan.mode = "custom";
  return `custom curve set: ${formatCurve(points)}`;
};

const mockFanWatchdog = async (): Promise<boolean> => {
  await delay();
  return mockDaemon;
};
