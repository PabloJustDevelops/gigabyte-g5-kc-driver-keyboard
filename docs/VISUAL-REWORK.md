# Visual rework — the GPUI panel

Plan for making `g5kbd-gui` look like a finished product instead of a
functional port. Written 2026-10-03, before any code change. Nothing here has
been implemented yet; this document exists so the work does not have to be
rediscovered and so the decisions are recorded.

Scope: **`gui/` only**. No kernel, CLI, backend or packaging changes.

---

## 1. Where we are

The panel was rewritten from Tauri/React/Tailwind/shadcn to a single Rust
binary on GPUI + `gpui-kit` (`gpui-component`). The rewrite is merged and
green: `cargo fmt`, `clippy -D warnings`, 6 unit tests, CI `Panel (fmt +
clippy + test)`, and it launches on Wayland without crashing.

What has **not** happened: nobody has looked at it. The window has never been
opened by a human since the rewrite. Everything below is inferred from reading
the source, not from seeing the app.

> **Superseded by §10.** The panel has now been rendered and looked at, and
> §2's three causes turned out to be one wrong number, one missing shadow and
> one missing theme pin. The phases below are still the plan; §10 records what
> has actually landed.

Files that own the look:

| File | What it holds |
|---|---|
| `gui/src/theme.rs` | The palette (hex, ported from the old web `styles.css`), `mix`, `parse_hex`, `hex_of` |
| `gui/src/widgets.rs` | `Section` card, fan/temperature bars, the keyboard preview (`KB_ROWS`, `keyboard_preview`) |
| `gui/src/views.rs` | Sidebar, header, error banner, the four views, `Render for Panel` |
| `gui/src/panel.rs` | State and plumbing (not visual, but owns the live values) |

The old web stylesheet is recoverable and is the reference for the intended
look:

```sh
git show HEAD~4:gui/src/styles.css
```

---

## 2. Diagnosis — why it looks degraded

Three concrete causes. All three are fixable inside GPUI; none of them require
a new dependency.

### 2.1 The font does not exist on the machine

`views.rs` sets `.font_family("IBM Plex Sans")`. On this machine:

```sh
fc-list | grep -ci plex      # → 0
fc-match "IBM Plex Sans"     # → nothing
```

fontconfig therefore substitutes an arbitrary family, and the substitution can
differ per weight/feature. Result: inconsistent metrics, wrong weights, and a
layout that never quite lines up. This is not a design decision that was made
and rejected — it is an unported assumption from the web build (where the font
was bundled).

**Decision needed:** bundle IBM Plex Sans/Mono (OFL, ~1 MB) or drop to a system
family. See §7.

### 2.2 The decoration from the web CSS was not ported

The old `styles.css` carried most of the visual quality. The GPUI port has none
of it — the keyboard preview is a flat 45 % tint. Specifically missing:

| Web CSS | What it did | GPUI equivalent |
|---|---|---|
| `box-shadow: 0 0 7px 0 var(--glowcol)` | glow on keycaps, live dot, brand mark | `BoxShadow { blur_radius, spread_radius, .. }` |
| `box-shadow: inset 0 1px 0 rgba(255,255,255,0.05)` per key | the keycap's top highlight that makes it read as a 3D key | `BoxShadow { inset: true, .. }` |
| `box-shadow: inset 0 2px 14px rgba(0,0,0,0.55)` on `.keyboard` | recessed well the keys sit in | `BoxShadow { inset: true, .. }` |
| `linear-gradient(180deg, rgba(255,255,255,0.015), transparent 60%)` | faint sheen on panels | `linear_gradient()` + `linear_color_stop()` |
| `transition: background/border-color/box-shadow 110–120ms ease` | every hover/focus | `spring()` / `elements/animation.rs` |
| `.app.off .key { filter: brightness(0.5) }` | a real "off" state | `mix(colour, black, ..)` |
| `color-mix(in srgb, glow 26%, base)` key face | key tint that scales with brightness | `mix()` with the web's ratios |
| `color-mix(… glow 45% …)` key border | border tint | `mix()` |
| `color-mix(… white 55% …)` legend | legend brightens with glow | `mix()` |
| `@keyframes breathe` / `cycle` | live-effect approximation | `spring()` loop or a timer-driven redraw |

GPUI supports all of these. Confirmed in the pinned snapshot:
`gpui-pre-0.3.7/src/style.rs:349` (`BoxShadow`, with the `inset` field),
`src/color.rs:865/893` (`linear_gradient` / `linear_color_stop`),
`src/style.rs:377` (`blur_radius`), `src/spring.rs` +
`src/elements/animation.rs` (the kit's own `Slider` thumb ring already uses
`spring()`).

### 2.3 Two colour systems collide

`gui/src/theme.rs` holds the hex palette ported from the web build. Separately,
`gpui-component` ships its own theme tokens
(`gpui-component-0.7.0/src/theme/theme_color.rs`, driven by
`Theme::change(ThemeMode::Dark, None, cx)`). The ready-made widgets (Button,
Slider, Input) render from the kit's tokens; our bespoke pieces render from
`theme.rs`. The two sets of greys, borders and radii are *almost* equal, so the
result is the worst outcome: it looks misaligned rather than deliberately
different.

**Decision needed:** converge on one source of truth. See §5.

---

## 3. What we have to work with

- **Only dependency is `gpui-kit = "0.7.0"`** (`gpui-pre 0.3.7` underneath).
  This is the de-facto standard component library for GPUI and by far the most
  used (167,918 downloads for `gpui-component` 0.7.0). There is no reason to
  change frameworks; the plan works *with* the kit.
- Already used from the kit: `Slider`, `Button`, `Input`, `ScrollableElement`,
  and the `IntoElement` derive macro.
- Deliberately not used, and that is fine: `ColorPicker` (we do RGB sliders +
  hex + presets), `Switch`/`ToggleGroup` (we do custom mode chips).
- GPUI styling is utility-method based, so most of the plan is "add the missing
  pairs to existing chains". `gpui-styling` (a skill, not a crate) documents
  Tailwind-like utilities for GPUI and would speed this up.

---

## 4. Non-goals

- No new GUI frameworks, no switching to `gpui-ce`.
- No image assets beyond the existing icons; the keyboard stays drawn as
  keycaps, not a photograph.
- No change to the four views' information architecture — only how it is
  presented.
- No backend/CLI/kernel/packaging edits.

---

## 5. Phase 1 — one token system

Establish a single set of design tokens and make both our code *and* the kit's
widgets render from it.

- Move the palette out of loose `pub const`s into a small token struct:
  semantic colours (`bg`, `surface`, `surface2`, `field`, `line`, `line_strong`,
  `text`, `muted`, `faint`, `ok`, `warn`, `danger`, `accent`), plus
  **spacing scale**, **radii scale**, **type scale** and **elevation levels**.
- Align the kit's theme with ours, or read the kit's tokens where the kit
  renders the widget (`Theme::change(ThemeMode::Dark, ..)` + whatever override
  the kit exposes) so a `Slider` and a bespoke `Section` sit on the same greys.
- Add the **real font**: one family for UI text, one monospace for readouts
  (hex, temperatures, duty). If bundled, ship the `.ttf`/`.otf` and register it
  with the app instead of relying on fontconfig.
- Define the type scale explicitly (the old CSS ran 7.5–40 px with real intent:
  a 30–40 px readout, 13.5 px body, 10.5 px uppercase labels, 11 px mono
  readouts). Today the panel leans on `text_sm`/`text_xs`/`text_xl` and loses
  that hierarchy.

Deliverable: `theme.rs` (or a `tokens.rs`) as the single source of truth, and a
note in the code saying the kit is deliberately aligned to it.

---

## 6. Phases 2–6

### Phase 2 — the keyboard preview (`widgets.rs`)

The preview is the app's centrepiece and the biggest gap.

- Per-key `inset` top highlight (`inset 0 1px 0 rgba(255,255,255,0.05)`), so a
  key reads as a key.
- Glow on lit keys from the web's `box-shadow: 0 0 calc(3px + p*12px)
  calc(p*1.5px)`, i.e. `blur_radius` and `spread_radius` driven by brightness —
  this is the effect that makes the panel look alive in the web version.
- Tinting at the web's ratios: face `mix(base, accent, 0.26*p)`, border
  `mix(line, accent, 0.45*p)`, legend `mix(legend_off, white, 0.55*p)` — instead
  of today's flat 0.45.
- A real off state: `mix(colour, black, ..)` darkening the whole board instead
  of only zeroing the glow.
- Recessed well: inset shadow on the `.keyboard` panel itself.
- Legend size: today `8px` is genuinely illegible at the rendered key size;
  revisit height/font-size together.
- Keep `KB_ROWS` as-is (the layout is correct, Ñ included).

### Phase 3 — chrome (`views.rs`, `widgets.rs`)

- **Sidebar**: a real active indicator. The web used
  `background: color-mix(accent 11%, panel)` + `border 1px accent 32%` + an
  accent icon; today it is a flat `PANEL2` fill with accent text. Add the
  background mix, the hairline border, and a left indicator bar.
- **Section cards**: elevation + the faint top sheen gradient; a "dominant"
  variant with more padding and a stronger border for the page's main choice.
- **Hover / active / focus**: the web had 110–120 ms transitions on every
  interactive surface; `gpui-component` has states but our bespoke pieces have
  none. Add hover/focus-visible affordances to sidebar items, mode chips, rows.
- **Scrollbar**: `.overflow_y_scrollbar()` exists; verify it is not visually
  jarring against the panel background and style it if the kit allows.
- **Error banner**: currently hard-coded `rgb(0x5c2430)`/`rgb(0x241318)` hexes
  in `views.rs` — pull them into tokens (`danger` tints).

### Phase 4 — motion

- 110–120 ms ease on hover/active/focus, matching the web's timing.
- A short view-switch transition (the web changed pages instantly, but the
  content max-width and the header did change; a subtle fade is enough).
- A pulsing live dot when an effect runs (`breathe`), instead of a static dot.
- Spring feedback on slider thumb release (the kit already springs the ring;
  check ours feels the same).
- Respect reduced motion — the web had an explicit
  `@media (prefers-reduced-motion: reduce)` reset. Find the GPUI equivalent, or
  gate animations behind a setting.

### Phase 5 — states and density

- Per-view empty / loading / error states. Today the fans view can show nothing
  while the first `g5fan status` runs; the web had honest "loading" rows.
- Density: Overview is a readout strip and should be dense; Fans is an
  instrument panel and can be looser. The web encoded this
  (`.page-inner` 720 px vs `.page-inner.wide` 1080 px). Reproduce it.
- The curve editor is **fixed at 5 points** (known limitation, already noted in
  `ROADMAP.md`). Out of scope here, but the visual plan should not make fixing
  it harder.

### Phase 6 — accessibility

- The 8 px keycap legends and the 9–10.5 px uppercase labels need a real
  contrast and size check.
- Keyboard focus order and visible focus rings on every interactive element
  (the web had `:focus-visible` with an accent outline).
- AccessKit labels on the icon-only and colour-only controls (swatches,
  keycaps, bars) — a screen reader currently sees nothing meaningful.
- Do not encode state in colour alone (temperature bars, mode chips).

---

## 7. Open questions (need answers before Phase 1)

1. **What bothers you most?** The keyboard preview, the chrome, or
   typography + density. This sets the order.
2. **Font**: bundle IBM Plex Sans/Mono (OFL, ~1 MB added to the package) or use
   a system family. Bundling is the only way to get the web look back on every
   machine.
3. **How much personality?** The same neutral near-black chrome done properly,
   or something more distinctive (this machine is a gaming laptop — a slightly
   more opinionated accent treatment is defensible).

---

## 8. How to verify without eyes on the window

**Solved — see §10.** `gui/tests/snapshot.rs` renders the panel offscreen with
GPUI's headless renderer and writes one PNG per view:

```sh
cd gui && cargo test --test snapshot -- --nocapture
# -> gui/target/snapshots/{home,lighting,profiles,fans}.png
```

This is the desktop equivalent of a browser test runner's screenshot step: no
display, no compositor, no window manager, identical output every run, and it
also runs in CI.

For "is the *real* window all right", `grim` still captures the live session
(Wayland/wlroots); `niri msg --json windows` reports the window geometry so the
capture can be cropped.

---

## 9. Suggested order

```
Phase 0   audit + install gpui-styling + pick snapshot tool   (can start now)
Phase 1   single token system + real font
Phase 2   keyboard preview
Phase 3   chrome (sidebar, sections, hover/focus, scrollbar)
Phase 4   motion
Phase 5   states + density
Phase 6   accessibility
```

Phases 2 and 3 are independent of each other and of 4–6. Phase 1 blocks all of
them, because everything else consumes the tokens.

---

## 10. Status — the panel has been looked at

The snapshot harness (§8) renders the real `Panel` at its real window size, so
the diagnosis above could be checked instead of guessed. It found four concrete
defects, three of them objective bugs rather than taste:

| Found | Fix |
|---|---|
| Content overflowed the pane and was clipped ("ceiling 10…", "not a…", "5 p…" on the Fans view). `max_w` capped the column but did not stop wide children — the gauge row and the curve help text pushed past the window. | Pin the column with `w_full` before `max_w`, let the gauge and curve rows wrap, and drop the gauge `min_w` from 180 to 148 px. |
| The keyboard rendered as a flat light-grey slab. The web panel mixed the LED colour into the key face at **26 %**; the port used 45 %, which turns a white keyboard into grey. | Restore the web's ratios (face 26 %, border 45 %, legend towards white) and add the un-ported decoration: per-keycap inset highlight, a recessed inset shadow on the board, and a brightness-driven glow. |
| Key widths were percentages *plus* inter-key gaps, so a 16-key row overflowed its card by the width of all its gaps. | `flex_grow(k.u)` with `flex_basis(0)` — the row now divides the space it actually has. |
| **Real bug, not cosmetic:** the component widgets follow the desktop's light/dark setting, so on a light-themed desktop every `Button` and `Input` rendered white inside our dark cards. | `theme::apply_kit_theme` pins the kit to `ThemeMode::Dark` and realigns its slider tokens. Called from `main.rs` and from the snapshot harness so both agree. |

The slider needed care: `slider_bar` is the *filled* part of the track, not the
groove. Its dark default is near-white, which made a 3 px line the loudest
thing on the Lighting view. It is now the accent, with a white thumb.

Still open, in plan order: the missing font (§7.2 — `IBM Plex Sans` is not on
this machine and has never been bundled), motion (Phase 4), states and density
(Phase 5) and accessibility (Phase 6).
