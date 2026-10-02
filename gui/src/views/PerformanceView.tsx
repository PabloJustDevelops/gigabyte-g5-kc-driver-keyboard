import { useCallback, useEffect, useRef, useState } from "react";
import { Fan, ShieldCheck, ShieldOff, Thermometer } from "lucide-react";
import { Section } from "../components/Section";
import { Slider } from "../components/ui/slider";
import * as api from "../lib/api";
import type { CurvePoint, FanMode, FanStatus } from "../lib/types";
import { cn } from "../lib/cn";

/* The five modes the Windows Control Center exposes (its oem.ini calls them
 * 0:Auto 1:Max 3:Silent 5:MAXQ 6:Custom — "Max" is labelled Turbo in the UI).
 * Auto and Turbo drive the duty directly; Silent, MaxQ and Custom are curves
 * the `g5fan` daemon evaluates against the current temperature. */
const MODES: { id: FanMode; label: string; desc: string }[] = [
  { id: "auto", label: "Auto", desc: "firmware curve" },
  { id: "turbo", label: "Turbo", desc: "full speed" },
  { id: "silent", label: "Silent", desc: "quiet curve" },
  { id: "maxq", label: "MaxQ", desc: "quietest curve" },
  { id: "custom", label: "Custom", desc: "your curve" },
];

const CURVE_MODES: FanMode[] = ["silent", "maxq", "custom"];

/** First thing wrong with a curve, or null. Mirrors validate_curve in
 * src/g5fan.py so the panel refuses exactly what the CLI refuses. */
function curveProblem(curve: CurvePoint[]): string | null {
  if (curve.length < 2) return "A curve needs at least two points.";
  for (let i = 1; i < curve.length; i++) {
    if (curve[i][0] <= curve[i - 1][0]) {
      return `T${i + 1} (${curve[i][0]} °C) must be above T${i} (${curve[i - 1][0]} °C).`;
    }
    if (curve[i][1] < curve[i - 1][1]) {
      return `D${i + 1} (${curve[i][1]}%) must not be below D${i} (${curve[i - 1][1]}%) — a fan must not slow down as it gets hotter.`;
    }
  }
  return null;
}

export function PerformanceView() {
  const [st, setSt] = useState<FanStatus | null>(null);
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState<string | null>(null);
  const [manual, setManual] = useState(50);

  const refresh = useCallback(async () => {
    try {
      const s = await api.fanStatus();
      setSt(s);
      setErr(s.error ?? null);
    } catch (e) {
      setErr(String(e));
    }
  }, []);

  useEffect(() => {
    void refresh();
    const t = setInterval(() => void refresh(), 3000);
    return () => clearInterval(t);
  }, [refresh]);

  const run = async (fn: () => Promise<unknown>) => {
    setBusy(true);
    setErr(null);
    try {
      await fn();
      await refresh();
    } catch (e) {
      setErr(String(e));
    } finally {
      setBusy(false);
    }
  };

  const setMode = (m: FanMode) => run(() => api.fanSetMode(m));

  const isManual = st?.mode === "manual";
  const currentManual = (isManual ? st?.manualDuty : null) ?? manual;
  const setDuty = (pct: number) => run(() => api.fanSetManual(pct));

  const curveMode = st !== null && CURVE_MODES.includes(st.mode as FanMode);
  const age = st ? Math.round(st.ageS) : 0;
  // `daemon` is written by the daemon itself, so it says "true" even after it
  // died: it is the snapshot's age that tells the two apart. Either signal
  // means the numbers below are the last ones it saw, not what is happening.
  const daemonDown = st !== null && (st.stale || !st.daemon);

  return (
    <>
      <Section title="Fans" readout={st ? st.mode : "loading…"}>
        {err && <p className="plan-note fan-err">{err}</p>}

        <div className="modes fan-modes" role="group" aria-label="Fan mode">
          {MODES.map((m) => (
            <button
              key={m.id}
              type="button"
              disabled={busy}
              className={cn("mode", st?.mode === m.id && "active")}
              aria-pressed={st?.mode === m.id}
              onClick={() => setMode(m.id)}
            >
              <span className="mode-name">{m.label}</span>
              <span className="mode-desc">{m.desc}</span>
            </button>
          ))}
        </div>

        {st && st.fans.length > 0 && (
          <div className="fan-readout">
            {st.fans.map((f) => (
              <div key={f.label} className="fan-gauge">
                <div className="fan-gauge-head">
                  <Fan className="fan-ico" strokeWidth={1.8} />
                  <span>{f.label}</span>
                </div>
                <div className="fan-gauge-value">
                  {f.duty ?? 0}
                  <small>%</small>
                </div>
                <div className="fan-gauge-sub">
                  {f.rpm ? `${f.rpm} rpm` : "stopped"}
                </div>
                <div className="fan-bar" aria-hidden="true">
                  <i style={{ width: `${f.duty ?? 0}%` }} />
                </div>
              </div>
            ))}

            {st.cpuTempC !== null && (
              <TempGauge
                label="CPU"
                temp={st.cpuTempC}
                ceiling={st.ceilingC}
              />
            )}
            {st.gpuTempC !== null && (
              <TempGauge label="GPU" temp={st.gpuTempC} ceiling={st.ceilingC} />
            )}
          </div>
        )}

        {daemonDown && (
          <p className="plan-note fan-err">
            The fan daemon is not running, so nothing is evaluating the curves
            — the values above are the last ones it published
            {age > 0 ? ` (${age} s ago)` : ""}. Start it with:{" "}
            <code>sudo systemctl start g5fan-watchdog.service</code>
          </p>
        )}
        {st && !daemonDown && curveMode && (
          <p className="plan-note">
            {st.mode} is driven by the daemon: it samples every 5 s and hands
            both fans back to the firmware curve at {Math.round(st.ceilingC)}{" "}
            °C. Updated {age} s ago.
          </p>
        )}
        {st && !st.driver && (
          <p className="plan-note">
            The <code>g5kbd</code> module is not loaded, so commands go through
            the raw EC (<code>ec_sys</code>) instead of the firmware's own
            method. Both paths write the same registers; load it with{" "}
            <code>sudo modprobe g5kbd</code> to use the driver.
          </p>
        )}
      </Section>

      <Section
        title="Manual duty"
        readout={isManual ? `${currentManual}%` : "not active"}
      >
        <div className="fan-manual">
          <Slider
            value={currentManual}
            min={0}
            max={100}
            step={1}
            disabled={busy}
            ariaLabel="Manual fan duty, percent"
            onValueChange={setManual}
            onValueCommit={(v) => void setDuty(v)}
          />
          <div className="action-row">
            <button
              type="button"
              className="btn"
              disabled={busy}
              onClick={() => void setDuty(manual)}
            >
              Pin at {manual}%
            </button>
            <button
              type="button"
              className="btn"
              disabled={busy}
              onClick={() => void run(() => api.fanSetMode("auto"))}
            >
              Back to auto
            </button>
          </div>
        </div>
        <p className="plan-note">
          Pinning the duty takes the fans away from the firmware curve. The fan
          daemon hands them back if the CPU gets too hot.
        </p>
      </Section>

      {st && <FanCurve st={st} busy={busy} run={run} />}
    </>
  );
}

function TempGauge({
  label,
  temp,
  ceiling,
}: {
  label: string;
  temp: number;
  ceiling: number;
}) {
  return (
    <div className="fan-gauge">
      <div className="fan-gauge-head">
        <Thermometer className="fan-ico" strokeWidth={1.8} />
        <span>{label}</span>
      </div>
      <div className={cn("fan-gauge-value", temp >= ceiling - 10 && "hot")}>
        {temp.toFixed(0)}
        <small>°C</small>
      </div>
      <div className="fan-gauge-sub">
        {ceiling > 0 ? `release at ${Math.round(ceiling)} °C` : ""}
      </div>
      <div className="fan-bar" aria-hidden="true">
        <i
          style={{
            width: `${ceiling > 0 ? Math.min(100, (temp / ceiling) * 100) : 0}%`,
          }}
        />
      </div>
    </div>
  );
}

function FanCurve({
  busy,
  st,
  run,
}: {
  busy: boolean;
  st: FanStatus;
  run: (fn: () => Promise<unknown>) => Promise<void>;
}) {
  // The curve this editor edits is always the *custom* one; the named presets
  // come from the CLI (`presets` in `g5fan status --json`), so the panel does
  // not carry a second copy of them that could drift.
  const remote = api.formatCurve(st.customCurve);
  const [curve, setCurve] = useState<CurvePoint[]>(st.customCurve);
  const dirty = useRef(false);

  useEffect(() => {
    // Re-seed when the stored curve changes, but never while the user is in
    // the middle of editing one — the poll must not overwrite their sliders.
    if (!dirty.current) setCurve(st.customCurve);
  }, [remote]); // eslint-disable-line react-hooks/exhaustive-deps

  const edit = (next: CurvePoint[]) => {
    dirty.current = true;
    setCurve(next);
  };

  const problem = curveProblem(curve);
  const usingCustom = st.mode === "custom";
  // Edited but not saved, and whether there is anything left to do at all
  // (a curve that matches the stored one still has to be *selected*).
  const unsaved = api.formatCurve(curve) !== api.formatCurve(st.customCurve);
  const canApply = unsaved || !usingCustom;

  const set = (i: number, which: 0 | 1, v: number) =>
    edit(
      curve.map((p, j) =>
        j === i
          ? ([which === 0 ? v : p[0], which === 1 ? v : p[1]] as CurvePoint)
          : p,
      ),
    );

  const apply = () =>
    run(async () => {
      await api.fanSetCurve(curve);
      dirty.current = false;
      // Saving the curve does not select it: the daemon only drives whichever
      // mode is selected, so applying means switching to Custom as well.
      if (!usingCustom) await api.fanSetMode("custom");
    });

  return (
    <Section
      title="Fan curve"
      readout={
        unsaved ? "unsaved" : usingCustom ? "in use" : `${curve.length} points`
      }
    >
      <p className="plan-note">
        The ramp is evaluated by the fan daemon: at or below T1 the fans hold
        D1, at or above T{curve.length} they hold D{curve.length}, and between
        two points the duty is interpolated. Temperatures must rise and duty
        must <em>never</em> fall — a curve that dips is how a CPU cooks.
      </p>

      <div className="fan-curve">
        {curve.map(([t], i) => (
          <label key={`t${i}`} className="fan-curve-field">
            <span>
              T{i + 1} <small>°C</small>
            </span>
            <Slider
              value={t}
              min={20}
              max={110}
              step={1}
              disabled={busy}
              ariaLabel={`Point ${i + 1} temperature in °C`}
              onValueChange={(v) => set(i, 0, Math.round(v))}
            />
            <output>{t}</output>
          </label>
        ))}
      </div>

      <div className="fan-curve">
        {curve.map(([, d], i) => (
          <label key={`d${i}`} className="fan-curve-field">
            <span>
              D{i + 1} <small>%</small>
            </span>
            <Slider
              value={d}
              min={0}
              max={100}
              step={1}
              disabled={busy}
              ariaLabel={`Point ${i + 1} duty in percent`}
              onValueChange={(v) => set(i, 1, Math.round(v))}
            />
            <output>{d}</output>
          </label>
        ))}
      </div>

      {problem && <p className="plan-note fan-err">{problem}</p>}

      <div className="action-row fan-curve-row">
        <button
          type="button"
          className="btn"
          disabled={busy || problem !== null || !canApply}
          onClick={() => void apply()}
        >
          {usingCustom ? "Apply curve" : "Save and use"}
        </button>
        <button
          type="button"
          className="btn"
          disabled={busy}
          onClick={() => edit(st.presets.custom ?? st.customCurve)}
        >
          Reset to the shipped curve
        </button>
        <button
          type="button"
          className="btn"
          disabled={busy || curve.length >= 5}
          title="Add a point before the hottest one"
          onClick={() =>
            edit([
              ...curve.slice(0, -1),
              [
                Math.round((curve[curve.length - 2][0] + curve[curve.length - 1][0]) / 2),
                Math.round((curve[curve.length - 2][1] + curve[curve.length - 1][1]) / 2),
              ] as CurvePoint,
              curve[curve.length - 1],
            ])
          }
        >
          Add point
        </button>
        <button
          type="button"
          className="btn"
          disabled={busy || curve.length <= 2}
          title="Remove the second-to-last point"
          onClick={() => edit(curve.filter((_, i) => i !== curve.length - 2))}
        >
          Remove point
        </button>
      </div>

      <p className="plan-note">
        Applying stores the curve and selects <strong>Custom</strong>; the
        daemon picks it up within a few seconds. The built-in Silent and MaxQ
        curves stay as shipped.
      </p>
    </Section>
  );
}

/** Small status strip so the watchdog is never a silent unknown. */
export function WatchdogChip() {
  const [on, setOn] = useState<boolean | null>(null);
  useEffect(() => {
    let alive = true;
    const check = async () => {
      try {
        const v = await api.fanWatchdog();
        if (alive) setOn(v);
      } catch {
        if (alive) setOn(null);
      }
    };
    void check();
    const t = setInterval(() => void check(), 10000);
    return () => {
      alive = false;
      clearInterval(t);
    };
  }, []);

  if (on === null) return null;
  return (
    <span className={cn("fan-watchdog-chip", on ? "on" : "off")}>
      {on ? <ShieldCheck size={13} /> : <ShieldOff size={13} />}
      daemon {on ? "on" : "off"}
    </span>
  );
}
