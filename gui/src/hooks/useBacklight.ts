import { useCallback, useEffect, useRef, useState } from "react";
import {
  brightnessRaw,
  effectStart,
  effectStop,
  getBackend,
  getState,
  profileApply,
  profileDelete,
  profileSave,
  profilesList,
  setBrightnessPct,
  setColor,
  setPower,
} from "../lib/api";
import type { DeviceState, EffectMode, Profile, StatusKind } from "../lib/types";
import type { Rgb } from "../lib/color";

export interface Status {
  msg: string;
  kind: StatusKind;
}

const initialDevice: DeviceState = {
  enabled: true,
  red: 0,
  green: 0,
  blue: 200,
  brightness: 100,
  effectRunning: false,
  backend: "…",
};

const recentWindow = 600; // ms: don't let a poll clobber a fresh local commit

export function useBacklight() {
  const [dev, setDev] = useState<DeviceState>(initialDevice);
  const devRef = useRef(dev);
  devRef.current = dev;

  const [mode, setMode] = useState<EffectMode | null>(null);
  const modeRef = useRef(mode);
  modeRef.current = mode;

  const [speed, setSpeed] = useState(5);
  const speedRef = useRef(speed);
  speedRef.current = speed;

  const [status, setStatusMsg] = useState<Status>({ msg: "ready", kind: "" });
  const [profiles, setProfiles] = useState<Profile[]>([]);
  const [selected, setSelected] = useState<string | null>(null);

  const localAt = useRef<{ color: number; brightness: number }>({
    color: 0,
    brightness: 0,
  });
  const dragging = useRef(false);
  const rawLast = useRef(0);
  const profilesSig = useRef("");
  const statusTimer = useRef<number | undefined>(undefined);
  const selectedRef = useRef<string | null>(selected);
  selectedRef.current = selected;

  const noteLocal = (k: "color" | "brightness") => {
    localAt.current[k] = Date.now();
  };
  const recentLocal = (k: "color" | "brightness") =>
    Date.now() - localAt.current[k] < recentWindow;

  /* ── status (auto-clears unless an effect is running) ──── */
  const setStatus = useCallback((msg: string, kind: StatusKind = "") => {
    setStatusMsg({ msg, kind });
    if (statusTimer.current !== undefined) window.clearTimeout(statusTimer.current);
    if (!devRef.current.effectRunning) {
      statusTimer.current = window.setTimeout(
        () => setStatusMsg({ msg: "ready", kind: "" }),
        6000,
      );
    }
  }, []);

  /* ── poll the CLI-backed state file ────────────────────── */
  const refresh = useCallback(async () => {
    try {
      const st = await getState();
      setDev((prev) => {
        const next = { ...st };
        if (dragging.current) {
          // keep whatever the user is mid-gesture on
          next.red = prev.red;
          next.green = prev.green;
          next.blue = prev.blue;
          next.brightness = prev.brightness;
          next.enabled = prev.enabled;
        } else {
          if (recentLocal("color")) {
            next.red = prev.red;
            next.green = prev.green;
            next.blue = prev.blue;
          }
          if (recentLocal("brightness")) next.brightness = prev.brightness;
        }
        return next;
      });
    } catch (e) {
      if (!devRef.current.effectRunning) setStatus(String(e), "err");
    }
  }, [setStatus]);

  const reloadProfiles = useCallback(async () => {
    try {
      const list = await profilesList();
      const sig = list
        .map((p) => `${p.name}\t${p.color}\t${p.brightness}\t${p.state}`)
        .join("\n");
      if (sig !== profilesSig.current) {
        profilesSig.current = sig;
        setProfiles(list);
        setSelected((sel) =>
          sel && !list.some((p) => p.name === sel) ? null : sel,
        );
      }
    } catch (e) {
      setStatus("profiles: " + String(e), "err");
    }
  }, [setStatus]);

  useEffect(() => {
    void getBackend()
      .catch(() => "led")
      .then((b) => setDev((p) => ({ ...p, backend: b })));
    void refresh();
    void reloadProfiles();
    const t1 = window.setInterval(refresh, 1000);
    const t2 = window.setInterval(reloadProfiles, 2000);
    return () => {
      window.clearInterval(t1);
      window.clearInterval(t2);
      if (statusTimer.current !== undefined) window.clearTimeout(statusTimer.current);
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  /* ── colour ────────────────────────────────────────────── */
  const previewColor = useCallback((rgb: Rgb) => {
    setDev((prev) => ({ ...prev, red: rgb.r, green: rgb.g, blue: rgb.b }));
    noteLocal("color");
  }, []);

  const commitColor = useCallback(
    async (rgb: Rgb) => {
      previewColor(rgb);
      try {
        setStatus(await setColor(rgb), "ok");
      } catch (e) {
        setStatus(String(e), "err");
      }
      void refresh();
    },
    [previewColor, refresh, setStatus],
  );

  /* ── brightness (live raw write while dragging) ────────── */
  const liveBrightness = useCallback((pct: number) => {
    const v = Math.min(100, Math.max(0, Math.round(pct)));
    setDev((prev) => ({ ...prev, brightness: v }));
    noteLocal("brightness");
    const now = Date.now();
    if (now - rawLast.current > 80) {
      rawLast.current = now;
      brightnessRaw(Math.round((v / 100) * 255)).catch(() => {});
    }
  }, []);

  const commitBrightness = useCallback(
    async (pct: number) => {
      const v = Math.min(100, Math.max(0, Math.round(pct)));
      setDev((prev) => ({ ...prev, brightness: v }));
      noteLocal("brightness");
      try {
        setStatus(await setBrightnessPct(v), "ok");
      } catch (e) {
        setStatus(String(e), "err");
      }
      void refresh();
    },
    [refresh, setStatus],
  );

  /* ── power ─────────────────────────────────────────────── */
  const togglePower = useCallback(async () => {
    const on = !devRef.current.enabled;
    setDev((prev) => ({ ...prev, enabled: on }));
    try {
      setStatus(await setPower(on), "ok");
    } catch (e) {
      setStatus(String(e), "err");
    }
    void refresh();
  }, [refresh, setStatus]);

  /* ── effects ───────────────────────────────────────────── */
  const pickMode = useCallback((m: EffectMode) => {
    if (devRef.current.effectRunning) return;
    setMode((cur) => (cur === m ? null : m));
  }, []);

  const changeSpeed = useCallback((n: number) => {
    setSpeed(Math.min(10, Math.max(1, Math.round(n))));
  }, []);

  const startEffect = useCallback(async () => {
    const m = modeRef.current;
    if (!m || devRef.current.effectRunning) return;
    try {
      setStatus(await effectStart(m, speedRef.current), "ok");
      setDev((prev) => ({ ...prev, effectRunning: true }));
    } catch (e) {
      setStatus(String(e), "err");
    }
    void refresh();
  }, [refresh, setStatus]);

  const restartEffectAtSpeed = useCallback(
    async (n: number) => {
      changeSpeed(n);
      if (devRef.current.effectRunning && modeRef.current) {
        try {
          setStatus(await effectStart(modeRef.current, n), "ok");
        } catch (e) {
          setStatus(String(e), "err");
        }
        void refresh();
      }
    },
    [changeSpeed, refresh, setStatus],
  );

  const stopEffect = useCallback(async () => {
    try {
      setStatus(await effectStop(), "ok");
      setDev((prev) => ({ ...prev, effectRunning: false }));
    } catch (e) {
      setStatus(String(e), "err");
    }
    void refresh();
  }, [refresh, setStatus]);

  /* ── profiles ──────────────────────────────────────────── */
  const applyProfile = useCallback(
    async (name: string) => {
      setSelected(name);
      try {
        setStatus(await profileApply(name), "ok");
      } catch (e) {
        setStatus(String(e), "err");
      }
      void refresh();
    },
    [refresh, setStatus],
  );

  const saveProfile = useCallback(
    async (rawName: string) => {
      let name = rawName.trim();
      if (!name) name = `profile ${profiles.length + 1}`;
      try {
        setStatus(await profileSave(name), "ok");
      } catch (e) {
        setStatus(String(e), "err");
      }
      void reloadProfiles();
    },
    [profiles.length, reloadProfiles, setStatus],
  );

  const deleteProfile = useCallback(
    async (explicitName?: string) => {
      const name = explicitName ?? selectedRef.current;
      if (!name) return;
      setSelected(null);
      try {
        setStatus(await profileDelete(name), "ok");
      } catch (e) {
        setStatus(String(e), "err");
      }
      void reloadProfiles();
    },
    [reloadProfiles, setStatus],
  );

  return {
    dev,
    mode,
    speed,
    status,
    profiles,
    selected,
    dragging,
    setDragging: (v: boolean) => {
      dragging.current = v;
    },
    setStatus,
    refresh,
    previewColor,
    commitColor,
    liveBrightness,
    commitBrightness,
    togglePower,
    pickMode,
    changeSpeed,
    restartEffectAtSpeed,
    startEffect,
    stopEffect,
    applyProfile,
    saveProfile,
    deleteProfile,
    reloadProfiles,
  };
}

export type Backlight = ReturnType<typeof useBacklight>;
