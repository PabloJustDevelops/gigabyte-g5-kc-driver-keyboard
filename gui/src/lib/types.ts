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
