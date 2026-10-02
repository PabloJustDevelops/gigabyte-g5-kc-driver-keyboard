export type EffectMode = "breathe" | "cycle";
export type StatusKind = "" | "ok" | "err";

export interface DeviceState {
  enabled: boolean;
  red: number;
  green: number;
  blue: number;
  /** 0..100 */
  brightness: number;
  effectRunning: boolean;
  backend: string;
}

export interface Profile {
  name: string;
  color: string; // RRGGBB
  brightness: number; // 0..100
  state: string; // "on" | "off"
}

/** Fan modes, mirroring the Windows module's oem.ini ids. */
export type FanMode = "auto" | "turbo" | "silent" | "maxq" | "custom";

/** One point of the fan curve: a temperature in °C and a duty in %. */
export type CurvePoint = [number, number];

export interface FanReading {
  /** "CPU" | "GPU" */
  label: string;
  /** the duty the EC is commanding, 0..100, or null */
  duty: number | null;
  /** RPM, or null */
  rpm: number | null;
  /** the tachometer period behind `rpm`, or null */
  tacho: number | null;
}

export interface FanStatus {
  /** the selected mode: "auto" | "turbo" | "manual" | silent | maxq | custom */
  mode: string;
  /** "kernel", "ec" or "fake" — which layer answers the writes */
  backend: string;
  /** true when the kernel driver is loaded (false = raw-EC fallback) */
  driver: boolean;
  /** the duty a manual mode pinned, 0..100, or null */
  manualDuty: number | null;
  /** the curve actually in play, or null unless a curve mode is selected */
  curve: CurvePoint[] | null;
  /** the saved custom curve, whether or not it is the one in play */
  customCurve: CurvePoint[];
  /** the built-in presets, keyed by curve mode (silent / maxq / custom) */
  presets: Record<string, CurvePoint[]>;
  fans: FanReading[];
  /** EC CPU temperature in °C, or null */
  cpuTempC: number | null;
  /** EC GPU temperature in °C, or null */
  gpuTempC: number | null;
  /** the CPU's own high-temperature threshold in °C the daemon releases at */
  ceilingC: number;
  /** whether the daemon behind g5fan-watchdog.service is running the curves */
  daemon: boolean;
  /** seconds since the fan daemon published this reading */
  ageS: number;
  /** true when the daemon looks dead: these are the last numbers it saw */
  stale: boolean;
  /** set when the panel could not read the fans at all */
  error?: string;
}
