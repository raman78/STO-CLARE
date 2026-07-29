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

| Session | Back end | Mechanism |
|---|---|---|
| Wayland | **wlr-layer-shell** surface on its own thread | `src/app/overlay/layer_shell.rs` |
| X11, Windows, macOS | eframe **deferred viewport** | `show_viewport_deferred` with `.with_always_on_top()` |

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

| Variant | Payload | Effect on the layer thread |
|---|---|---|
| `Data` | `OverlayData` | replace displayed rows, request redraw |
| `Move` | `bool` | toggle move mode → swap input region |
| `Stop` | — | leave the event loop, tear down (I1) |

A closed channel (`ChannelEvent::Closed`) is treated as `Stop`.

## Components

| File / symbol | Responsibility | Called by |
|---|---|---|
| `overlay/mod.rs` `Overlay` | app-side controller, back-end selection, UI buttons | main tab UI |
| `overlay/mod.rs` `OverlayInner` | polls analyzer, builds `DisplayData`, owns the `LayerOverlay` handle | `Overlay` |
| `overlay/layer_shell.rs` `LayerOverlay` | thread handle: `spawn`/`update`/`set_move`/`stop`; stops thread on `Drop` | `OverlayInner` |
| `overlay/layer_shell.rs` `run()` | thread body: Wayland globals, event loop, redraw loop | `spawn` |
| `overlay/layer_shell.rs` `State` | per-surface state: wgpu, egui, geometry, pointer/drag | delegated handlers |
| `custom_widgets/table.rs` `Table` | shared table widget used by both back ends | both render paths |

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

| Symptom | Cause | Where to look |
|---|---|---|
| Segfault when hiding/closing the overlay | I1 violated (surface dropped before wgpu) | field order in `State`, `app.gpu = None` in `run()` |
| `layer overlay: ...` logged, no overlay | Wayland connect/bind failed (not a Wayland session, no layer-shell) | `run()` return path, `spawn()` error log |
| Overlay eats clicks meant for the game | input region left non-empty (I3) | `apply_input_region()`, move-mode plumbing |
| Overlay stuck at 240×80 or oversized | auto-size not converging | `render()` size block, `Table::size()` |

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

| setting | written | read |
|---|---|---|
| `overlay_position` | every frame in `App::ui` (Linux) | when the overlay is next shown |
| `overlay_shown` | `App::on_exit` only | `Overlay::new`, straight into `OverlayInner.show` |

`overlay_shown` is written **only on exit**, not as the ✋/Overlay button is
toggled. `SettingsWindow::apply_setting_changes` compares the whole `general`
section against the live settings and re-analyzes the log when they differ, so a
value that changed mid-session would trigger a pointless re-analysis on the next
Apply. (`overlay_position` does update live and has exactly that quirk.)

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
