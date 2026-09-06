/* g5kbd-gui frontend
 *
 * Talks to the Rust core exclusively through `invoke()` (Tauri commands that
 * wrap the g5kbd CLI). When opened in a plain browser (no Tauri host, e.g.
 * `npm run dev` on http://localhost:5173) it falls back to an in-memory mock
 * so the UI can be previewed without the driver installed.
 */

import { invoke } from "@tauri-apps/api/core";

/* ── environment ─────────────────────────────────────────── */
const IS_TAURI = "__TAURI_INTERNALS__" in window;

/* ── dom helpers ─────────────────────────────────────────── */
const $ = (id) => document.getElementById(id);

const els = {
  backendDot: $("backend-dot"),
  backendName: $("backend-name"),
  kbRows: $("kb-rows"),
  fxNote: $("fx-note"),
  swatches: $("swatches"),
  hexInput: $("hex-input"),
  hexApply: $("hex-apply"),
  valR: $("val-r"), valG: $("val-g"), valB: $("val-b"),
  slR: $("sl-r"), slG: $("sl-g"), slB: $("sl-b"),
  slBright: $("sl-bright"), brightVal: $("bright-val"),
  btnPower: $("btn-power"),
  modeBreathe: $("mode-breathe"), modeCycle: $("mode-cycle"),
  slSpeed: $("sl-speed"), speedVal: $("speed-val"),
  btnEffect: $("btn-effect"), btnStop: $("btn-stop"),
  status: $("status"),
};

const PRESETS = [
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

/* ── state ───────────────────────────────────────────────── */
const ui = {
  red: 0, green: 0, blue: 200,
  brightness: 100,
  enabled: true,
  effectRunning: false,
  mode: "",            // "breathe" | "cycle"
  speed: 5,
  backend: "…",
  dragging: false,
};

/* ── api ─────────────────────────────────────────────────── */
async function call(cmd, args = {}) {
  if (IS_TAURI) return invoke(cmd, args);
  return mock(cmd, args);
}

/* tiny mock so the panel is previewable in a plain browser */
const mockState = { ...ui };
async function mock(cmd, args) {
  await new Promise((r) => setTimeout(r, 40));
  switch (cmd) {
    case "get_state":
      return { ...mockState, backend: "led (mock)" };
    case "backend": return "led (mock)";
    case "set_color":
      mockState.red = args.red; mockState.green = args.green; mockState.blue = args.blue;
      return `colour #${hex(mockState)}`;
    case "set_brightness":
      mockState.brightness = args.pct;
      return `brightness ${args.pct}%`;
    case "set_power":
      mockState.enabled = args.on;
      return args.on ? "on" : "off";
    case "effect_start": {
      mockState.effectRunning = true; mockState.mode = args.mode;
      return `effect ${args.mode} running`;
    }
    case "effect_stop":
      mockState.effectRunning = false; mockState.mode = "";
      return "stopped";
    default: return "mock ok";
  }
}

/* ── rendering helpers ───────────────────────────────────── */
const hex = (s) =>
  [s.red, s.green, s.blue].map((v) => v.toString(16).padStart(2, "0")).join("");

function buildKeyboard() {
  const rows = [
    Array(14).fill("k"),
    Array(14).fill("k"),
    Array(14).fill("k"),
    ["k", "k", "k", "k", "space", "k", "k", "k", "k"],
  ];
  els.kbRows.innerHTML = "";
  for (const row of rows) {
    const line = document.createElement("div");
    line.className = "kb-row";
    for (const kind of row) {
      const k = document.createElement("div");
      k.className = kind === "space" ? "key space" : "key";
      line.appendChild(k);
    }
    els.kbRows.appendChild(line);
  }
}

function buildSwatches() {
  els.swatches.innerHTML = "";
  for (const [col, name] of PRESETS) {
    const s = document.createElement("button");
    s.className = "swatch";
    s.style.setProperty("--sw", "#" + col);
    s.title = name;
    s.dataset.hex = col;
    s.addEventListener("click", () => applyColor(col));
    els.swatches.appendChild(s);
  }
  markSelectedSwatch();
}

function markSelectedSwatch() {
  const cur = hex(ui);
  for (const s of els.swatches.children) {
    s.classList.toggle("sel", s.dataset.hex === cur);
  }
}

/* body classes drive the preview look + lock state */
function paint() {
  const rgb = `${ui.red} ${ui.green} ${ui.blue}`;
  document.body.style.setProperty("--glowcol", `rgb(${rgb})`);
  document.body.style.setProperty("--glowp", String(ui.brightness / 100));
  document.body.classList.toggle("off", !ui.enabled);
  document.body.classList.toggle("lit", ui.enabled && ui.brightness > 0);
  document.body.classList.toggle("locked", ui.effectRunning);
  document.body.classList.toggle("fx-breathe", ui.effectRunning && ui.mode === "breathe");
  document.body.classList.toggle("fx-cycle", ui.effectRunning && ui.mode === "cycle");
  els.fxNote.classList.toggle("hidden", !(ui.effectRunning && ui.mode));

  const keys = els.kbRows.querySelectorAll(".key");
  keys.forEach((k) => k.classList.toggle("lit", ui.enabled && ui.brightness > 0));

  els.slR.value = ui.red; els.slG.value = ui.green; els.slB.value = ui.blue;
  els.valR.textContent = ui.red; els.valG.textContent = ui.green; els.valB.textContent = ui.blue;
  els.slBright.value = ui.brightness;
  els.brightVal.textContent = ui.brightness + "%";
  els.slSpeed.value = ui.speed;
  els.speedVal.textContent = ui.speed;
  els.hexInput.value = hex(ui);

  els.btnPower.textContent = ui.enabled ? "on" : "off";
  els.btnPower.setAttribute("aria-pressed", String(ui.enabled));

  els.modeBreathe.classList.toggle("active", ui.mode === "breathe");
  els.modeCycle.classList.toggle("active", ui.mode === "cycle");

  const controls = [els.slR, els.slG, els.slB, els.slBright, els.hexApply, els.btnPower, els.hexInput];
  controls.forEach((c) => { c.disabled = ui.effectRunning; });
  els.swatches.classList.toggle("locked-ui", ui.effectRunning);
  els.modeBreathe.disabled = ui.effectRunning;
  els.modeCycle.disabled = ui.effectRunning;

  els.btnEffect.disabled = ui.mode === "" || ui.effectRunning;
  els.btnStop.disabled = !ui.effectRunning;
  els.btnEffect.textContent = ui.mode ? `run ${ui.mode} @ ${ui.speed}` : "pick a mode";
  markSelectedSwatch();
}

/* ── status ──────────────────────────────────────────────── */
let statusTimer = null;
function setStatus(msg, kind = "") {
  els.status.textContent = msg;
  els.status.className = "status" + (kind ? " " + kind : "");
  clearTimeout(statusTimer);
  statusTimer = setTimeout(() => {
    if (!ui.effectRunning) els.status.className = "status";
  }, 6000);
}

/* ── actions (each goes through the g5kbd CLI) ───────────── */
async function applyColor(hexCol) {
  const [r, g, b] = [0, 2, 4].map((i) => parseInt(hexCol.slice(i, i + 2), 16));
  ui.red = r; ui.green = g; ui.blue = b;
  paint();
  try {
    setStatus(await call("set_color", { red: r, green: g, blue: b }), "ok");
  } catch (e) {
    setStatus(e, "err");
  }
  markSelectedSwatch();
  refresh();
}

// Local-change timestamps so the poller doesn't snap a control back while a
// write is still in flight (the state file updates ~200 ms after release).
const localAt = { brightness: 0 };
const recentLocal = (k) => Date.now() - (localAt[k] || 0) < 600;

async function applyBrightness() {
  ui.brightness = +els.slBright.value;
  paint();
  try {
    setStatus(await call("set_brightness", { pct: ui.brightness }), "ok");
  } catch (e) { setStatus(e, "err"); }
  localAt.brightness = Date.now();
  refresh();
}

async function togglePower() {
  ui.enabled = !ui.enabled;
  paint();
  try {
    setStatus(await call("set_power", { on: ui.enabled }), "ok");
  } catch (e) { setStatus(e, "err"); }
  refresh();
}

async function startEffect() {
  if (!ui.mode) return;
  try {
    const msg = await call("effect_start", { mode: ui.mode, speed: ui.speed });
    ui.effectRunning = true;
    setStatus(msg, "ok");
  } catch (e) { setStatus(e, "err"); }
  paint();
}

async function stopEffect() {
  try {
    const msg = await call("effect_stop");
    ui.effectRunning = false;
    setStatus(msg, "ok");
  } catch (e) { setStatus(e, "err"); }
  paint();
  refresh();
}

/* ── polling ─────────────────────────────────────────────── */
async function refresh() {
  try {
    const st = await call("get_state");
    if (!ui.dragging) {
      ui.red = st.red; ui.green = st.green; ui.blue = st.blue;
      ui.enabled = st.enabled;
      if (!recentLocal("brightness")) ui.brightness = st.brightness;
    }
    ui.effectRunning = st.effect_running;
    // if an effect is running but we don't know its mode (e.g. started from
    // the CLI after the GUI opened), still show the approximate motion note
    ui.backend = st.backend || "…";
    els.backendName.textContent = ui.backend;
    els.backendDot.className =
      "dot" + (String(ui.backend).startsWith("led") ? " live" : String(ui.backend).includes("mock") ? " live" : " ec");
    paint();
  } catch (e) {
    /* keep last state; surface only when idle */
    if (!ui.effectRunning) setStatus(e, "err");
  }
}

/* ── wiring ──────────────────────────────────────────────── */
function wire() {
  els.slR.addEventListener("input", () => { ui.red = +els.slR.value; paint(); });
  els.slG.addEventListener("input", () => { ui.green = +els.slG.value; paint(); });
  els.slB.addEventListener("input", () => { ui.blue = +els.slB.value; paint(); });

  const commitRgb = () => {
    ui.red = +els.slR.value; ui.green = +els.slG.value; ui.blue = +els.slB.value;
    applyColor(hex(ui));
  };
  els.slR.addEventListener("change", commitRgb);
  els.slG.addEventListener("change", commitRgb);
  els.slB.addEventListener("change", commitRgb);

  // Live feel: while dragging, push the raw level straight to the LED sysfs
  // node (no CLI spawn, throttled). On release the CLI call persists state.
  let lastLiveBright = 0;
  els.slBright.addEventListener("input", () => {
    ui.brightness = +els.slBright.value;
    els.brightVal.textContent = ui.brightness + "%";
    paint();
    const now = performance.now();
    if (now - lastLiveBright > 80) {
      lastLiveBright = now;
      call("brightness_raw", { level: Math.round(ui.brightness * 2.55) }).catch(() => {});
    }
  });
  els.slBright.addEventListener("change", () => {
    lastLiveBright = 0;
    applyBrightness();
  });

  els.btnPower.addEventListener("click", togglePower);

  els.hexInput.addEventListener("input", () => {
    els.hexInput.classList.remove("bad");
    const m = els.hexInput.value.match(/^[0-9a-fA-F]{6}$/);
    if (m) { ui.red = parseInt(m[0].slice(0, 2), 16); ui.green = parseInt(m[0].slice(2, 4), 16); ui.blue = parseInt(m[0].slice(4, 6), 16); paint(); }
    else if (els.hexInput.value.length === 6) els.hexInput.classList.add("bad");
  });
  els.hexApply.addEventListener("click", () => {
    if (/^[0-9a-fA-F]{6}$/.test(els.hexInput.value)) applyColor(els.hexInput.value.toLowerCase());
    else { els.hexInput.classList.add("bad"); setStatus("hex must be 6 hex digits", "err"); }
  });
  els.hexInput.addEventListener("keydown", (e) => {
    if (e.key === "Enter" && /^[0-9a-fA-F]{6}$/.test(els.hexInput.value)) applyColor(els.hexInput.value.toLowerCase());
  });

  const pickMode = (mode) => {
    if (ui.effectRunning) return;
    ui.mode = mode;
    paint();
    els.btnEffect.textContent = `run ${mode} @ ${ui.speed}`;
  };
  els.modeBreathe.addEventListener("click", () => pickMode("breathe"));
  els.modeCycle.addEventListener("click", () => pickMode("cycle"));

  els.slSpeed.addEventListener("input", () => {
    ui.speed = +els.slSpeed.value;
    els.speedVal.textContent = ui.speed;
    paint();
  });
  els.slSpeed.addEventListener("change", () => {
    // changing speed while running restarts the effect at the new speed
    if (ui.effectRunning && ui.mode) startEffect();
  });

  els.btnEffect.addEventListener("click", startEffect);
  els.btnStop.addEventListener("click", stopEffect);

  /* drag guard: don't let the poller fight a slider mid-drag */
  ["slR", "slG", "slB", "slBright"].forEach((id) => {
    els[id].addEventListener("pointerdown", () => { ui.dragging = true; });
    els[id].addEventListener("pointerup", () => { ui.dragging = false; });
    els[id].addEventListener("change", () => { ui.dragging = false; });
  });
}

/* ── boot ────────────────────────────────────────────────── */
buildKeyboard();
buildSwatches();
wire();
paint();

(async function boot() {
  try {
    ui.backend = await call("backend");
  } catch { ui.backend = "led"; }
  await refresh();
  setInterval(refresh, 1000);
})();
