# Combat overlay

## Purpose

The overlay is the always-on-top DPS/metrics panel a player keeps in front of
the game while fighting. It mirrors the newest combat and must float above a
full-screen game window. This document covers how it is rendered, why there are
two rendering back ends, and the invariants that keep the Wayland back end from
crashing.

Owned by `src/app/overlay/mod.rs` (`Overlay`, the app-side controller) and
`src/app/overlay/layer_shell.rs` (`LayerOverlay`, the Wayland surface). Driven
from the main tab UI via `Overlay::show()`.

## Context

The app is a single `eframe`/`egui` process (`src/app/mod.rs`). The main window
runs on the main thread. The overlay needs to be a *separate* top-level surface
that stays above the game.

```
                          main eframe/egui process
   ┌───────────────────────────────────────────────────────────┐
   │  App (main window)                                         │
   │    └─ Overlay  ── Arc<Mutex<OverlayInner>>                 │
   │          │                                                 │
   │   session split in Overlay::show()                         │
   │          │                                                 │
   │   ┌──────┴───────────────┐                                 │
   │   │                      │                                 │
   │  X11 / Windows /      Wayland                              │
   │  macOS                LayerOverlay ── calloop channel ───► │
   │  eframe deferred      (handle)                             │
   │  viewport                                                  │
   └───────────────────────────────────────────────────────────┘
                                                    │
                              cla-layer-overlay thread (Wayland only)
                              wlr-layer-shell surface + wgpu + egui
```

## Why two back ends

The split exists because on **Wayland** a normal top-level window (what an
eframe viewport is, via `winit`) cannot force itself above a full-screen game:
the always-on-top hint is advisory and compositors (KWin included) ignore it.
The `wlr-layer-shell` protocol's `overlay` layer *is* honored, so there the
overlay is a layer surface instead of a viewport. The two back ends render the
same content (see [Unified styling](#unified-styling)) but share no windowing
code.

| Session             | Back end                                      | Mechanism                                             |
|---------------------|-----------------------------------------------|-------------------------------------------------------|
| Wayland             | **wlr-layer-shell** surface on its own thread | `src/app/overlay/layer_shell.rs`                      |
| X11, Windows, macOS | eframe **deferred viewport**                  | `show_viewport_deferred` with `.with_always_on_top()` |

## Why the choice is made at runtime

`layer_shell` is compiled on Linux (`#[cfg(target_os = "linux")]`), but which
back end is *used* is decided while the app runs, in
`OverlayInner::uses_layer_shell()`. A `cfg` on the target OS would be wrong:
one Linux binary has to serve both session types, and wlr-layer-shell is a
Wayland protocol with nothing to connect to under X11 — picking it there would
leave the Overlay button doing nothing at all, while the plain always-on-top
viewport works fine in an X11 session.

The signal is the shared wgpu handles: `App::new` only injects them when
`is_wayland()` finds a `RawDisplayHandle::Wayland` on eframe's
`CreationContext`, which reports the back end `winit` actually chose rather than
a guess. `uses_layer_shell()` is then simply "were the handles injected", so the
two can never disagree, and a session without them falls back to the viewport
instead of failing. `main.rs` makes the same call one step earlier — before any
window exists, so from `WAYLAND_DISPLAY` / `WAYLAND_SOCKET`, the way winit does
it — to decide whether the shared wgpu stack is worth building at all.

The selected back end is logged once at startup (`overlay backend: ...`).

## Invariants

- **I1 — Drop order (Wayland).** In `run()` the wgpu surface (`State.gpu`) is
  built from raw handles of the `wl_surface`/`Connection`. It MUST be dropped
  before them, or wgpu tears down a surface backed by a destroyed object and
  the process segfaults. Enforced two ways: `gpu` is the first field of `State`
  (fields drop in declaration order, `overlay/layer_shell.rs`) and `run()`
  explicitly sets `app.gpu = None` before returning (`overlay/layer_shell.rs`).
- **I2 — Non-zero surface size.** Wayland rejects a 0×0 geometry. The surface
  never requests below `MIN_W`×`MIN_H` (240×80); the auto-size clamps to it.
- **I3 — Passthrough by default.** Out of move mode the surface carries an
  empty input region, so clicks reach the game underneath. See
  [Move mode](#move-mode-and-input-passthrough).
- **I4 — All overlay-thread data is `Send` + plain.** Only `OverlayData`
  (`Vec<String>` columns + rows) crosses the channel; no `egui`/`Combat` types.

## Data flow

Combat data originates in the analyzer and reaches the overlay through an
`AnalysisHandler` (a per-consumer subscription to refreshed combats). With the
layer-shell back end the formatted snapshot is then handed to the layer thread
over a calloop channel.

```
 analyzer ─► AnalysisHandler ─► OverlayInner.poll_update()   (overlay/mod.rs)
                                     │  AnalysisInfo::Refreshed
                                     ▼
                                OverlayInner.perform_update() (overlay/mod.rs)
                                     │  build DisplayData (sorted rows,
                                     │  formatted strings, enabled columns)
                                     ▼
 layer-shell:  OverlayInner.to_overlay_data()
                                     │  OverlayData { columns, rows } (plain)
                                     ▼
                LayerOverlay.update(data)  ── Msg::Data ──► calloop channel
                LayerOverlay.set_move(f)   ── Msg::Move ──►      │
                                                                 ▼
                                            cla-layer-overlay thread: State
```

`Overlay::show()` pumps this every frame while the overlay is visible and asks
the main context to repaint every 500 ms (`overlay/mod.rs`) so fresh data keeps
flowing to the thread. `set_move` is sent every frame too; the thread ignores
it unless the flag actually changed (`overlay/layer_shell.rs`, `Msg::Move` handler).

### Message contract

`enum Msg` (`overlay/layer_shell.rs`) is the only thing crossing the thread boundary:

| Variant | Payload       | Effect on the layer thread             |
|---------|---------------|----------------------------------------|
| `Data`  | `OverlayData` | replace displayed rows, request redraw |
| `Move`  | `bool`        | toggle move mode → swap input region   |
| `Stop`  | —             | leave the event loop, tear down (I1)   |

A closed channel (`ChannelEvent::Closed`) is treated as `Stop`.

## Components

| File / symbol                           | Responsibility                                                            | Called by          |
|-----------------------------------------|---------------------------------------------------------------------------|--------------------|
| `overlay/mod.rs` `Overlay`              | app-side controller, back-end selection, UI buttons                       | main tab UI        |
| `overlay/mod.rs` `OverlayInner`         | polls analyzer, builds `DisplayData`, owns the `LayerOverlay` handle      | `Overlay`          |
| `overlay/layer_shell.rs` `LayerOverlay` | thread handle: `spawn`/`update`/`set_move`/`stop`; stops thread on `Drop` | `OverlayInner`     |
| `overlay/layer_shell.rs` `run()`        | thread body: Wayland globals, event loop, redraw loop                     | `spawn`            |
| `overlay/layer_shell.rs` `State`        | per-surface state: wgpu, egui, geometry, pointer/drag                     | delegated handlers |
| `custom_widgets/table.rs` `Table`       | shared table widget used by both back ends                                | both render paths  |

The `LayerOverlay` handle lives in `OverlayInner.layer`
(`Option<LayerOverlay>`). It is created lazily on first visible frame
(`overlay/mod.rs`) and dropped when the overlay is hidden
(`toggle_show()`, `overlay/mod.rs`), which stops the thread.

## Layer thread internals (Wayland)

`run()` (`overlay/layer_shell.rs`) sets up a self-contained Wayland client using
`smithay-client-toolkit` (SCTK):

1. Connect, bind globals: `CompositorState`, `LayerShell`, `Shm`, `SeatState`.
2. Create a surface, wrap it as an `overlay`-layer `LayerSurface` anchored
   `TOP | LEFT`, keyboard interactivity `None`.
3. Set the initial (empty) input region via `apply_input_region()` (I3).
4. Build a calloop `EventLoop`, feed it the Wayland source and the `Msg`
   channel receiver.
5. Loop: `dispatch(16 ms)` then `render()` when `needs_redraw`; exit on `stop`.
6. On exit, drop `gpu` explicitly (I1).

### wgpu surface from raw handles

egui needs a wgpu surface, but SCTK owns the `wl_surface`. `State::init_gpu()`
(`overlay/layer_shell.rs`) bridges them: it reads the `wl_display` pointer from
the connection backend and the `wl_surface` pointer from the layer, wraps them
as `raw-window-handle` `Wayland*Handle`s, and calls
`Instance::create_surface_unsafe`. This is the coupling that makes I1 mandatory.

```rust
// src/app/overlay/layer_shell.rs  (init_gpu, elided)
let display_ptr = NonNull::new(self.conn.backend().display_ptr() as *mut _)...;
let surface_ptr = NonNull::new(self.layer.wl_surface().id().as_ptr() as *mut _)...;
let raw_display = RawDisplayHandle::Wayland(WaylandDisplayHandle::new(display_ptr));
let raw_window  = RawWindowHandle::Wayland(WaylandWindowHandle::new(surface_ptr));
// instance.create_surface_unsafe(RawHandle { raw_display, raw_window })
```

### Render loop and auto-size

`State::render()` (`overlay/layer_shell.rs`) runs egui headless
(`egui::Context::run` with a manual `RawInput` screen rect), tessellates, and
submits via `egui_wgpu::Renderer`. `pixels_per_point` is fixed at `1.0`.

The surface sizes itself to its content: the `Table` returns its rect, and the
required size (plus margins) is clamped to `MIN_W`×`MIN_H` (I2). When it differs
from the current size, `render()` calls `layer.set_size()` + `commit()` and
requests another redraw. Because the `Table` measures column widths over a few
frames, `render()` also forces a redraw while
`egui_ctx.has_requested_repaint()` is true, so the size settles.

### Failure modes

| Symptom                                  | Cause                                                               | Where to look                                       |
|------------------------------------------|---------------------------------------------------------------------|-----------------------------------------------------|
| Segfault when hiding/closing the overlay | I1 violated (surface dropped before wgpu)                           | field order in `State`, `app.gpu = None` in `run()` |
| `layer overlay: ...` logged, no overlay  | Wayland connect/bind failed (not a Wayland session, no layer-shell) | `run()` return path, `spawn()` error log            |
| Overlay eats clicks meant for the game   | input region left non-empty (I3)                                    | `apply_input_region()`, move-mode plumbing          |
| Overlay stuck at 240×80 or oversized     | auto-size not converging                                            | `render()` size block, `Table::size()`              |

## Move mode and input passthrough

`LayerSurface` cannot be dragged like an xdg-toplevel, so movement is
implemented with the input region + surface margins.

- **Input region** (`apply_input_region()`, `overlay/layer_shell.rs`): in move
  mode the whole surface takes pointer input (`set_input_region(None)`); out of
  move mode an empty `Region` is attached so clicks fall through (I3).
- **Pointer** is obtained through `SeatHandler` when a seat advertises the
  pointer capability; `PointerHandler::pointer_frame`
  (`overlay/layer_shell.rs`) tracks position and left-button drag.
- **Drag** (`drag_to()`, `overlay/layer_shell.rs`): on left-button press the
  surface-local pointer position becomes the `grab` point. Each motion adjusts
  the `(top, left)` margin by `pointer - grab`, then `set_margin` + `commit`.

Why margins from a fixed anchor rather than absolute placement: layer surfaces
have no client-set absolute position; the offset from an anchor is the only
positioning lever. The grab point stays valid across the move because after the
commit the surface origin has shifted by the same delta, so the next
surface-local pointer coordinate re-references to the original `grab` — the
margins self-correct rather than drift.

`move_mode` is driven entirely by the app: the ✋ button toggles
`OverlayInner.move_around`, which is pushed via `set_move()` each frame. It
starts `true` so a freshly shown overlay can be positioned before being locked
down.

## What is remembered across restarts

Two things persist, both in the `general` settings section:

| setting            | written                          | read                                              |
|--------------------|----------------------------------|---------------------------------------------------|
| `overlay_position` | every frame in `App::ui` (Linux) | when the overlay is next shown                    |
| `overlay_shown`    | `App::on_exit` only              | `Overlay::new`, straight into `OverlayInner.show` |

`overlay_shown` is written **only on exit**, not as the ✋/Overlay button is
toggled, so that a mid-session change cannot make the settings dialog think the
`general` section moved.

`overlay_position` *does* update every frame, and that used to be expensive:
`SettingsWindow::apply_setting_changes` compared `analysis` and `general`
together and, on any difference, both replaced the `Analyzer` and refreshed. So
nudging the overlay and then applying any setting re-parsed the whole log. The
two are now gated separately, because they cost very different amounts
(measured on a 150 MB log):

| call           | what it does                                                    | cost       |
|----------------|-----------------------------------------------------------------|------------|
| `set_settings` | builds a new `Analyzer` — full re-parse                         | **2.54 s** |
| `refresh`      | reuses it, reads only what the log grew by, re-sends the result | **2.8 µs** |

Only `analysis` can invalidate the analyzer (it is the only thing the analyzer is
ever given), so `set_settings` is now gated on that alone. A `general` change
still triggers `refresh`, because tables bake `more_decimals` into their
formatted strings when built (`ShieldAndHullTextValue::new`) rather than at draw
time — but that path is effectively free.

Restoring needs nothing beyond setting `show`: the render path in
`Overlay::show` branches on `inner.show` alone, and the analysis handler is
already created with auto-refresh on — the same state `toggle_show` would leave
behind for a visible overlay. `set_gpu` still runs first, because `App::new`
finishes before the first frame, so the layer-shell back end is selected
normally.

## Unified styling

Both back ends render with the same `Table` widget
(`src/custom_widgets/table.rs`) rather than duplicating layout. The layer
thread uses it inside its headless egui context exactly as the viewport path
does (`Table::new(ui).header(...).body(...)`), which is possible because
`eframe` re-exports the same `egui` crate the layer thread depends on directly
(a single `egui 0.34` in the lockfile). The header/body row heights (15/25) and
right-aligned value cells match the viewport overlay, so the two look
identical.

## The toolbar, and click-through on an ordinary window

Both back ends now carry ⛭ and ✋ **on the overlay itself**; nothing about the
overlay is left in the main window except the button that opens it.

On Wayland this is free: the surface declares an input region covering only the
toolbar, and the compositor routes every click itself (`apply_input_region`).
An ordinary window has no equivalent — winit exposes `set_cursor_hittest(bool)`
and egui `ViewportCommand::MousePassthrough(bool)`, both whole-window. So the
viewport back end flips that switch itself, from where the pointer is:

```
every ~50 ms, while the overlay is open:

  pointer position (pointer::on_screen)      toolbar rect (last frame)
              │                                        │
              └──────────────► is it inside? ◄─────────┘
                                    │
                    yes ────────────┴──────────── no
                     │                             │
        window takes the pointer          window is click-through
        (toolbar is clickable)            (clicks reach the game)
```

`pointer::on_screen` is the one thing winit cannot answer — while the overlay is
click-through it receives no pointer events at all — so it is one system call
per platform: `GetCursorPos` on Windows, `QueryPointer` on X11, and `None`
elsewhere (which leaves the overlay click-through unless it is being moved, the
behaviour it had before it had a toolbar). Both crates were already in the tree
via winit, so neither adds a dependency.

Two details that are easy to get wrong:

- **Wake the right context.** The builder lives in the main window, but the
  command is delivered to the overlay's viewport, which has to be awake to act
  on it. Repainting only the main window left the switch flipping seconds late;
  `request_repaint_after_for(.., viewport_id())` is what makes it prompt.
- **The ⛭ popup is outside the toolbar strip.** Its checkboxes would be
  unclickable under a toolbar-only rule, so the whole window takes the pointer
  while the popup is open — the same exception the layer-shell input region
  makes for `settings_open`.

The cost against Wayland is one poll interval: a click made in the very instant
the pointer crosses onto the toolbar can still go to the game. Measured with a
0.3 s dwell before clicking, the toolbar responds.

## Transparency

`settings.visuals.overlay_opacity` (0.2–1.0, default 0.85) decides how solid the
overlay is. Only the overlay is affected; the main window is a window like any
other.

The opacity is carried as **alpha on what the overlay paints**, not as a
window-manager opacity hint, so both back ends land on one mechanism:
`overlay::overlay_visuals` returns the theme's `Visuals` with every background
taken down to the chosen alpha — `panel_fill`, `faint_bg_color` (the stripe
under every other table row), `extreme_bg_color`, and each `WidgetVisuals`
`bg_fill`/`weak_bg_fill` behind the overlay's own toolbar buttons.

Covering only `panel_fill` is not enough and shows immediately: the panel goes
see-through while the row stripes stay solid, so the table looks like it is
floating on bars. `every_background_fades_not_just_the_panel` holds that line.

Text is deliberately **not** faded — `fg_stroke` and `override_text_color` are
left as the theme set them. A see-through overlay is meant to stop hiding the
game, not to stop being readable, and the figures are the whole point of it
(`the_figures_stay_readable_at_any_opacity`).

The floor of 0.2 (`overlay::MIN_OPACITY`) keeps the overlay findable: a fully
invisible one could not be clicked to switch off.

### Live preview

The settings dialog edits a working copy (`SettingsWindow::modified_settings`)
and only commits it on Ok, but a slider whose effect you cannot see until then
is guesswork. So while the dialog is open, `SettingsWindow::show` pushes the
working copy's opacity straight to the overlay each frame
(`Overlay::set_opacity`), the same way picking a theme repaints the app at once.

| Closing with | What happens                                                                                                                                                                                                                                |
|--------------|---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------|
| Ok           | `apply_setting_changes` sends the whole settings to the overlay. `visuals_changed` joins the condition for `settings_changed`, but deliberately **not** the one for `refresh`/`set_settings` — a colour has no business re-parsing the log. |
| Cancel       | `discard_setting_changes` puts the live value back, unconditionally: the preview ran regardless of what else the user touched.                                                                                                              |

`set_opacity` only writes the one field, so it stays cheap enough to call per
frame — unlike `settings_changed`, which clones the whole settings including the
rule lists.

The two back ends apply the visuals differently, because of where each renders:

| Back end    | How                                                                                                                                                                               |
|-------------|-----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------|
| layer-shell | The visuals go into the `Style` already pushed to the overlay thread (`LayerOverlay::set_style`), so the thread's own egui context draws everything at that opacity.              |
| viewport    | Renders inside the app's own context, so the visuals are set on the overlay's `Ui` (`*ui.visuals_mut() = …`) rather than pushed globally — the main window must not fade with it. |

Three things had to line up, and each was silently cancelling the alpha out:

| Where                     | What it was                       | Why it mattered                                                                                                                                                                                                                                                                                                         |
|---------------------------|-----------------------------------|-------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------|
| `layer_shell::init_gpu`   | `alpha_mode: caps.alpha_modes[0]` | KWin offers `[Opaque, PreMultiplied]` — index 0 is `Opaque`, which discards alpha wholesale. Now the mode is chosen by preference (`PreMultiplied`, then `PostMultiplied`, then `Inherit`), which is also what egui paints with. The negotiated mode is logged at startup.                                              |
| `layer_shell` render pass | `LoadOp::Clear` with `a: 0.85`    | It sat *under* the opaque panel egui then painted, so it never showed. The clear is now `Color::TRANSPARENT` and the panel carries the alpha.                                                                                                                                                                           |
| `App::clear_color`        | `window_fill()`, alpha 255        | eframe hands one clear colour to **every** viewport (`wgpu_integration.rs`: `app.clear_color(...)` per viewport), so the overlay window was wiped opaque before egui drew. Now `[0, 0, 0, 0]`. The main window is unaffected: its surface is opaque, so the alpha is ignored, and its central panel covers every pixel. |

The viewport back end additionally asks for `ViewportBuilder::with_transparent(true)`.
Whether that is honoured is up to the desktop — without a compositor the window
simply stays solid, which is a cosmetic loss, not a failure.

Measured on KWin/Wayland, sampling the overlay's surface through the compositor
at two settings:

| Back end              | opacity 1.0 | opacity 0.3                        |
|-----------------------|-------------|------------------------------------|
| layer-shell (Wayland) | 84,84,84    | 70,70,72 (dark desktop behind)     |
| viewport (XWayland)   | 80,80,80    | 200,200,200 (light desktop behind) |

## Testing

`overlay/layer_shell.rs` has one `#[ignore]` integration test, `spawn_render_stop`,
that spawns the overlay, pushes a row, toggles move mode both ways, and stops —
exercising the I1 teardown path and the input-region swap. It needs a real
Wayland session and briefly shows the overlay:

```
cargo test spawn_render_stop -- --ignored
```

## Decisions and trade-offs

- **Separate thread for the layer surface.** The layer surface runs its own
  Wayland connection and calloop loop instead of sharing the main `winit`
  event loop. Reason: eframe/winit does not expose layer-shell, and driving a
  second Wayland client on the main loop would entangle it with eframe's. Cost:
  a thread and the `Msg` channel; benefit: the two back ends stay fully
  decoupled and the main loop is untouched.
- **Plain `OverlayData` over the channel (I4).** Formatting and sorting happen
  on the main side (`perform_update`), so the layer thread only lays out
  strings. Keeps `egui`/`Combat`/`Settings` off the thread boundary.
- **Margins for movement, not absolute coordinates.** Forced by the protocol;
  see [Move mode](#move-mode-and-input-passthrough).

## Open questions

1. Multi-output placement — the surface is not pinned to a chosen monitor; the
   compositor picks the output. Needs an output-selection story if users run
   multiple monitors. Not blocking current single-overlay use.
