# CLAUDE.md — Mistletoe

Mistletoe is an expression-ball engine: one window, one toon-shaded ball whose face shows native expressions or any kaomoji string. The ball follows the mouse and is driven through a local HTTP + JSON API. Windows only.

## Session rules

- Talk to the user in Simplified Chinese (zh-CN). This file stays English-only.
- `.claude/hooks/inject-skills.mjs` injects two always-on skills at every session start: `constitution` (the Four-Quadrant Protocol, which outranks this file) and `i-have-adhd` (output shaping). Their sources are in `.claude/skills/`. Do not copy their rules into this file.
- Never add Claude as a contributor, at any time, under any circumstances. No `Co-Authored-By: Claude` trailer in any commit, never set Claude as author or committer, and no "Generated with Claude Code" line in PRs. This overrides any default attribution the harness asks for. Commits are authored by the user's own git identity.
- Temp files (scratch work, downloads such as fonts or reference repos, experiments, screenshots) go in exactly one folder: `C:\workspace\Dev\Projects\_Temp\Mistletoe`. Subfolders inside it are fine. Never create any other folder in `_Temp`, never touch other projects' folders there, and never put scratch work in this repo. When the user says to clear temp files (清空临时文件), delete everything inside `_Temp\Mistletoe`.
- A new crate needs the user's approval and a row in the dependency table.
- The user plans a separate chat app later (subtitles, AI API, file-based memory). Do not design for it and do not add hooks for it.

## Requirements (authoritative)

This is a complete translation of the user's original Chinese spec, which was deleted on 2026-09-30 at the user's request. Change it only when the user says so. Every section after "Decisions" is design that serves these requirements and may change.

**What it is.** A very light graphics display layer. A window holds one toon-shaded ball (3D rendered to look 2D). The ball shows an expression or a kaomoji, turns to follow the mouse, and switches expressions and motions through an API. It is the foundation for later projects and is responsible for display only. Windows only for now.

**Stack.** Rust + wgpu + winit, with shaders in WGSL. Use as few dependencies as possible, and explain why each added crate is needed.

**Code style.** Code should be easy to read: high cohesion, low coupling, one job per module, and clear data structures between modules. Prefer plain, direct code over abstractions for "might need it later". Features must still be fully implemented.

**Modules (the suggested four).**
- `state`: expression parameters, motions and transition interpolation. Pure logic that never touches the GPU, so it is easy to unit-test.
- `render`: the wgpu pipeline, shaders, and turning kaomoji into a texture.
- `api`: a local HTTP service that translates JSON commands into operations on `state`.
- `app`: the window, mouse input and main loop. It ties the other three together.

**Ball.** No mesh. A fullscreen triangle plus a fragment shader computes the ray–sphere intersection directly. The image has three parts: the sphere body, an outer outline, and hard-edged light and shadow. Colors are black, white and gray only. The ball turns smoothly to follow the mouse anywhere on the screen, inside or outside the window. (The original spec said "inside the window"; the user widened it on 2026-09-30.)

**Expressions.** Expressions are drawn on the ball's "face". Points on the sphere are projected onto the plane the face points toward, and the expression is drawn in that plane, so the face turns naturally with the ball. Native expressions:
- Eyes: several eye shapes. They blink randomly on their own and can also be made to blink through the API.
- Mouth: several mouth shapes with adjustable width, openness and curvature.
- Blush: gray diagonal lines. The palette is black, white and gray only, and lines read more like blushing than a gray patch would.
- Tears and sweat.
- Gloom ("dark face"): the gloomy look from manga, drawn only with black vertical lines over the upper face, no solid gray. (The original spec said "a dark shadow over the upper half of the face plus vertical lines"; the user changed it on 2026-10-01.)

**Kaomoji.** Any string must work, including combining characters such as `•̀`. Shape it with rustybuzz, read glyphs with ttf-parser, draw it into a grayscale texture, then cut hard edges in the shader. Bundle several OFL-licensed fonts as fallbacks so common kaomoji never miss glyphs.

**Motions and tags.** The ball can nod, shake its head, sway side to side, bounce and tremble. Each motion takes a duration and an intensity and is layered on top of mouse follow. A tag is an expression + motion combination: `happy` = smiling eyes + bounce, `no` = head shake, `scared` = sweat + tremble. The tag table lives in JSON and can be extended at any time. A kaomoji can carry a tag so the text and the motion match.

**API.** Local HTTP on 127.0.0.1, with JSON. The agent and the user use the same interface, with no extra back door, so what the user sees while debugging is exactly what the agent gets. Commands:
- `set_tag`: switch expression and motion by tag.
- `set_expression`: set expression parameters directly, with a smooth transition.
- `set_kaomoji`: show a kaomoji, optionally with a tag.
- `play_motion`: play one motion.
- `blink`: blink once.
- `get_state`: return the current state.
- `list_tags`: list all tags.

**Debugging aids.**
- Opening the server address in a browser shows a simple debug page: tag and motion buttons, a kaomoji input box, and a box for sending raw JSON. The page is an ordinary API client, compiled in with `include_str!`, with no added dependencies. It triggers and views only. No tuning sliders.
- The README gives a curl example for every command.
- Editing the tag table or the preset JSON reloads it automatically, with no restart.
- The console prints every received command and its result.

**Not doing.** A tuning editor, accessories, color, multiple balls, sound, auto-update, platforms other than Windows.

**Done when.**
1. It builds and runs on Windows.
2. Every expression, motion and tag can be triggered through the API. After a command, the transition starts within 100 ms.
3. Common kaomoji display correctly, with no missing glyphs and nothing misaligned.
4. The idle ball uses almost no resources.

## Decisions (made with the user, 2026-09-30)

1. Skills are project-scoped: this repo only.
2. Mistletoe runs as a standalone process. Other programs use only the HTTP API. It ships as a single binary crate.
3. The window is a normal resizable window with a title bar and a flat gray background. No transparency. It opens at 360×360 logical pixels, and the ball scales with the window's short side.
4. Chinese text must render too. Noto Sans SC is subset (GB2312 hanzi plus symbols) and embedded in the exe.
5. An expression holds until the next command. There is no auto-return to neutral.
6. A motion plays once by default. `loop: true` repeats it until it is replaced. A new motion replaces the old one; motions do not stack. `stop` is a pseudo-motion that fades the current motion out (added with the user on 2026-09-30, because a loop otherwise had no way to end).
7. A kaomoji replaces the eyes and mouth only. Blush, tears, sweat and gloom still show. `set_expression` or `set_tag` returns to the native face.
8. Tag names are English only (`happy`, `no`, `scared`, ...).
9. The API is a single endpoint: `POST /api` with `{"cmd": "...", ...}`.
10. The README, debug page and console logs are Chinese. Code comments are Chinese. This file is English.
11. Dependencies stay minimal (table below). HTTP, hot reload, RNG and logging are hand-written.

## Commands

```bash
cargo run -- --port 7777 --data data        # dev run; debug page at http://127.0.0.1:7777/
cargo build --release
cargo test                                  # state + api + font-coverage tests
cargo clippy --all-targets -- -D warnings
cargo fmt
python tools/subset_fonts.py                # only when changing fonts; needs `pip install fonttools`
curl -s -X POST http://127.0.0.1:7777/api -d '{"cmd":"set_tag","tag":"happy"}'
```

Toolchain verified on 2026-09-30: rustc 1.98.1, stable-x86_64-pc-windows-msvc, VS Build Tools at `C:\workspace\Dev\Runtime\VSBuildTools`.

## Dependencies

| Crate | Version | Why |
| --- | --- | --- |
| wgpu | 30.x | GPU access (required) |
| winit | 0.30.x | Window + input (required). 0.31 is beta; do not upgrade until it is stable. |
| pollster | 1.x | Blocks on wgpu's async adapter/device requests at startup |
| rustybuzz | 0.20.x | Text shaping (required). Use its re-export `rustybuzz::ttf_parser` for glyph outlines, which covers the ttf-parser requirement and keeps a single ttf-parser version in the tree. Do not add `ttf-parser` directly. |
| serde (derive) + serde_json | 1.x | JSON commands, tag table, presets |
| ab_glyph_rasterizer | 0.1.x | Turns glyph outlines into a coverage bitmap; zero dependencies |

Hand-written on purpose: the HTTP/1.1 server (`std::net`), hot reload (mtime polling), the RNG (xorshift), logging (`println!`), uniform-buffer byte packing (no bytemuck), and the Win32 `GetCursorPos` binding (no windows-sys).

## Code style

- Follow the Requirements code style: one job per module, plain data types between modules, no traits, generics, plugin points or config layers "for later".
- Comments are in Chinese. Every file starts with a Chinese header comment (`//!` in Rust, `//` in WGSL/JS, `<!-- -->` in HTML) that says what the file does and what it must not do. Add Chinese comments at key internal points: math, coordinate spaces, thread hand-offs, non-obvious constants. Identifiers, file names and commit messages are English.
- Never `unwrap()` on data from the network, the filesystem or JSON. Log a Chinese error and keep the previous state.
- Keep `cargo fmt` and `cargo clippy -- -D warnings` clean.

## Architecture

```
            HTTP thread                   main thread (winit event loop)
 client ──► api ──(Command, reply tx)──► app ──► state.apply / state.tick
                  via EventLoopProxy        │            │ Snapshot
 reload thread ──(parsed tables)──────────► │            ▼
                                            └──────► render.draw(&Snapshot)
```

`app` depends on `state`, `render` and `api`. `render` and `api` may use `state` types only. `state` depends only on std and serde. `render` never sees `api`, and `api` never sees `render` or winit.

```
src/main.rs        parse args (--port, --data), start app
src/state/         pure logic: no GPU, no I/O, no clock reads (time is passed in)
  command.rs       Command enum + Reply (serde, #[serde(tag = "cmd")])
  expression.rs    Expression, eye/mouth shapes, transitions
  blink.rs         random + commanded blinks (seeded xorshift)
  motion.rs        motion curves -> Pose offsets
  swap.rs          face-change squash → switch → pop timing, ball scale pulse
  follow.rs        mouse-follow target + smoothing
  tags.rs          presets + tag table, validation
src/render/        wgpu setup, the per-frame uniform, kaomoji texture
  ball.wgsl        fullscreen triangle, ray-sphere, outline, shading, face
  text.rs          shaping + font fallback + rasterization -> R8 bitmap
  fonts.rs         the embedded font chain (include_bytes!)
src/api/           local HTTP server; JSON <-> Command; command log
  http.rs          minimal request parsing / response writing
  debug.html       debug page, embedded with include_str!
src/app/           window, input, main loop, wiring
  cursor.rs        global cursor position (hand-written GetCursorPos FFI)
  reload.rs        polls data/*.json mtime every 1 s on its own thread
data/presets.json  named expressions + motion defaults (hot-reloaded)
data/tags.json     tag -> preset + motion (hot-reloaded)
assets/fonts/      bundled OFL fonts, each with its OFL.txt
tools/subset_fonts.py  dev-only font subsetting
```

### state

- The whole interface is `State::apply(cmd, now) -> Reply`, `State::tick(now) -> Snapshot` and `State::set_pointer(pointer) -> bool`. `now` is seconds since app start (`f64`). Tests drive them with fake time and a fixed RNG seed. `apply` validates everything before mutating, so an error leaves the state untouched.
- Until step 4 wires the API, `main.rs` puts `#[allow(dead_code)]` on `mod state` because only tests call `apply`. Remove it in step 4.
- `Expression` fields: `eyes` (`dot`, `smile`, `closed`, `squint`, `wide`, `sad`, `annoyed`); `mouth` { `shape` (`line`, `cat`, `triangle`, `wavy`, `grin`), `width` 0..1, `open` 0..1, `curve` -1..1 (negative = frown) }; overlays `blush`, `tears`, `sweat`, `gloom`, `bubble` (the sleepy snot bubble), each 0..1.
- Changing the face (prototype style, user's brief 2026-10-01; `state/swap.rs`): the whole face squashes vertically to zero over the first 40% of `duration_ms` (default 300, max 10000), every field (expression and kaomoji) switches at that midpoint, then the face pops back with a ~10% overshoot while the ball scales up by up to 6%. A change before the midpoint just updates what the midpoint switches to; a change after it runs another squash when the current one ends. `duration_ms: 0` switches at once. There is no per-field easing any more: overlay values are still 0..1 amounts, but they switch with the rest of the face.
- Blinks come at random intervals of 2–6 s and last about 150 ms. The `blink` command blinks now.
- Motions: `nod` (pitch), `shake` (yaw), `sway` (roll), `bounce` (vertical offset), `tremble` (fast deterministic jitter, no RNG: sines whose frequencies are rounded to whole cycles per loop, all under 30 Hz so 60 fps does not alias them). Each has `duration_ms` (50..60000), `intensity` (0..2, default from presets) and `loop`. A replaced motion blends out over 120 ms; several can be fading at once. Every curve starts and ends at zero so nothing pops. The motion pose is added on top of the mouse-follow pose.
- `Pose` is `yaw`, `pitch`, `roll` (radians) plus `x`, `y`, the ball-center offset in ball radii. `render` builds `R = Ry(yaw)·Rx(pitch)·Rz(roll)` and shifts the ball by `(x, y)·radius`.
- `set_tag` with a tag that has no motion leaves the current motion running, including a looping one. To end it, use `stop`: `MotionName::Stop` has no curve and no presets default, takes no parameters, and `Motion::play` only moves the current motion to the fade-out list. Tags may use it too (the built-in `neutral` tag does). `MotionName::ANIMATED` is the five real motions; `MotionName::ALL` adds `stop` for `list_tags`.
- Mouse follow works anywhere on the screen. `app` passes the cursor's offset from the window center in units of half the window's short side (unbounded outside the window). The target is `(yaw, pitch) = 30° · (x, y) / sqrt(x² + y² + 1)`: it always points toward the cursor, reaches about 71% of 30° at distance 1, and approaches but never exceeds 30° far away. Smoothing is exponential and frame-rate independent: `x += (target - x) * (1 - exp(-dt / 0.12))`. Once the error drops below 0.001 rad, snap to the target and stop.
- `Snapshot` carries `pose`, `face` (`expression` as shown, `blink` 0..1 closedness, `squish` 0..~1.1), `kaomoji` (shown text or none), `scale` (ball scale), `animating` and `next_wakeup` (the next random blink) so `app` can sleep.
- `get_state` reports the target expression plus the face and pose of the last rendered frame, so it shows what is on screen.
- `Command` uses empty struct variants (`Blink {}`) instead of unit variants so extra fields are rejected too.

### render

- Rendering follows the user's web prototype shader, `C:\Users\Alex\Downloads\prototype_reference.frag` (GLSL, outside the repo). Port its techniques to WGSL but keep the colors black, white and gray: outline and every line black, lit side near-white, shadow light gray.
- One fullscreen triangle. Orthographic rays along -Z hit the ball; `dist` is the ray-to-center distance in pixels. Outline: `dist` compared with `R` and `R·(1 + 0.04)`, both edges anti-aliased with `fwidth(dist)`. The ball's radius is 0.228 of the window's short side times the swap `scale`.
- Shading: `N·L` cut into two hard tones with `smoothstep(-0.012, 0.012, N·L)`. The light (`LIGHT_DIR`) points up-left and mostly toward the viewer, so the shadow is only a small crescent at the lower right. Rim light: `fres = 1 - N.z`, `smoothstep(0.62, 0.66, fres) · smoothstep(-0.05, 0.15, N·L)` paints a thin white ring inside the outline on the lit side only; the user calls it the key to the ball looking round. Because a white rim is invisible on white, the lit side is 0.92, not 1.0 (flagged to the user).
- Palette: `BACKGROUND` 0.55 (window), `LIT` 0.92, `SHADE` 0.72 (also the tongue), `WHITE` (rim, teeth, tear/sweat/bubble fill), `BLACK`. Surface is non-sRGB, so values are display bytes / 255. Interiors are flat; only edges blend through anti-aliasing.
- Face coordinates: `p = Rᵀ·N` is the normal in ball-local space; `q = p.xy / FACE_SCALE` is the face plane (orthographic), so features foreshorten naturally as the ball turns. `FACE_SCALE` is 1.55 so the prototype's own sizes (eye offset 0.26, eye size 0.1, line half-width 0.018, all in `q`) put the outer eye edges about half a ball diameter apart; at scale 1 they would be smaller than the user wants. `fclip = smoothstep(0.02, 0.22, p.z)` fades features softly toward the side of the ball. The swap squash divides `q.y` by `squish`, and the face is hidden below 0.04.
- Every part is a 2D SDF; `aaf(d) = 1 - smoothstep(-w, w, d)` with `w = 0.8·fwidth(d)`. WGSL only allows `fwidth` in uniform control flow, so `shade` and `draw_face` must not return early per pixel; helpers may branch internally. Feature colors ignore lighting.
- Arcs (smile eyes, mouth, wavy mouth) use `curve_sd`, a 12-segment polyline of `y = k·((x/w)² - 0.5) + wave·sin(1.5π·x/w)`, so ends are naturally round. Lines have constant width; the earlier tapered-brush idea was dropped for the prototype's round caps.
- Eyes use mirrored coordinates `e = (EYE_SPACING - |q.x - look.x|, ...)`, +x toward the nose, so one function draws both eyes. Blink only squashes eye y: filled shapes divide `e.y` by `max(open, 0.22)` so a closed eye is still a line; arc eyes scale their bulge by `open` so the line width stays constant; `wide` fills its white in black early in the blink so it never becomes a hollow frame. `sad` has a brow slanting down outward, `annoyed` a flat lid over a half-open eye and a brow slanting down toward the middle, `closed` is a deep ∪.
- Mouth: `line` is one arc; when open it cuts a black oval hole along the arc (O when narrow and unbent, D when smiling) with a gray tongue. `grin` is the prototype's D (half ellipse intersected with a line, outline via `abs`, teeth as `fract` lines clipped inside, white fill). `cat` is two small ∪ arcs, `triangle` is the prototype's filled triangle with a tongue, `wavy` is a smooth 1.5-period wave.
- Look offset: `extra.yz = (yaw, pitch) · 0.07`; eyes shift by it and the mouth by 0.6 of it, for a slight sense of depth.
- Overlays: blush is `fract` stripes clipped by an ellipse that grows with the amount; tears are rounded rectangles with a fixed sine wobble, white with a black outline, below the eyes with a gap, growing downward; sweat is two circles merged with `smin` (tip circle enlarged versus the prototype so the outlined drop has no thin neck), white with a black outline, at the right temple; bubble is a small outlined white circle.
- Gloom (rebuilt from scratch on 2026-10-01 after the user rejected a gray patch with hanging lines as looking like hair or a jellyfish): no gray at all, only thin black lines. Each line is a section `x = const` of the ball in local space, so it lies on the sphere, looks like a straight vertical line from the front and bends with the ball when it turns. Lines start above the top of the face and hang down; a long set reaches near the eyes and a short set between them only covers the top half, so the top is denser. Tops and bottoms vary per line (`hash`), widths rise slowly and taper to a point at the bottom. Lines over the eyes stop above them, and eyes, brows and mouth get a `GLOOM_HALO` clearance so they stay readable. Tried and rejected: meridian lines (they fan together at the top and read as bangs). Distances stay continuous: out-of-range lines get zero length instead of being skipped, because a jump to `FAR` makes `fwidth` draw a 1 px seam.
- No time-based effects in the shader (no `uTime` wobble or bob): the idle ball must not redraw. The prototype's idle sway/bob is pending the user's decision because it conflicts with "Done when" item 4.
- Eye and mouth shapes reach the shader as codes (`eye_code`, `mouth_code` in `render/mod.rs`) that must match the `switch` statements in `ball.wgsl`.
- Kaomoji: an R8 coverage texture sampled in face space with linear filtering, then `step(0.5, c)` for hard edges. Kaomoji mode hides the eyes and mouth. The text enters with a short scale-in, with no fade.
- Uniforms: a single `Globals` struct, packed by hand as little-endian bytes. Use only `f32`, `u32` and `vec4<f32>` fields to avoid WGSL alignment bugs. Current fields, all `vec4<f32>`: `screen` (w, h, radius px, outline fraction), `ball` (offset px), `face_u`, `face_v`, `face_f`, `expr` (eye code, eye openness 1..0, mouth code, squish), `mouth` (width, open, curve), `overlay` (blush, tears, sweat, gloom), `extra` (bubble, look x, look y).
- The kaomoji texture is re-rasterized only when the text changes.

### Kaomoji pipeline (render/text.rs)

1. Shape the whole string with the first font, with `Direction::LeftToRight` forced. Kaomoji mix in Thai and Arabic characters that must not flip.
2. Re-shape every cluster that produced glyph 0 with the next font in the chain. Fall back per cluster, never per char, so a base and its combining mark (`•̀`) stay in one font.
3. Feed each glyph outline through `ttf_parser::OutlineBuilder` into `ab_glyph_rasterizer` at ≥ 96 px/em, with padding. Lower resolutions make the thresholded edges wobble on a large ball.
4. Output is a single line. Scale the text to fit the face width. Cap the texture width at 2048 px by lowering px/em.

The font chain (all OFL, embedded, subset by `tools/subset_fonts.py`): Noto Sans → Noto Sans SC (GB2312 hanzi, kana, CJK punctuation, half- and fullwidth forms, box drawing, geometric shapes) → Noto Sans Symbols 2 → Noto Sans Math → Noto Sans Thai → Noto Sans Kannada → Noto Sans Canadian Aboriginal → Noto Sans Arabic → Noto Sans Tibetan. Change the list only through the coverage test. Choose the weight (Regular or Bold) after a visual check on the ball.

### api

- Binds `127.0.0.1` only, on default port 7777 (`--port`). If the port is taken, exit with a clear Chinese error.
- `GET /` serves the debug page, and `POST /api` takes one JSON command. Everything else returns 404.
- One accept loop runs on its own thread. It handles each connection inline with 2 s read/write timeouts and `Connection: close`. Bodies are limited to 64 KiB.
- Responses are `{"ok":true,"result":{...}}` or `{"ok":false,"error":"<Chinese message>"}`. Unknown fields and out-of-range values are errors, not silent clamps, so the user sees what the agent sees.
- The log line per command has the time, the command JSON, ok/error and the handling time.
- Hand-off: `app` gives `api` a `submit` closure (an EventLoopProxy send plus a one-shot `mpsc` reply channel with a 1 s timeout). `api` does not know winit.

| cmd | Fields | Effect |
| --- | --- | --- |
| `set_tag` | `tag` | Apply the tag's preset and motion, if present. Leaves kaomoji mode. |
| `set_expression` | any of `preset`, `eyes`, `mouth{shape,width,open,curve}`, `blush`, `tears`, `sweat`, `gloom`, `bubble`, `duration_ms` | Merge onto the current target and transition. Leaves kaomoji mode. |
| `set_kaomoji` | `text`, optional `tag` | Show the text in place of the eyes and mouth, and apply the tag. Empty `text` returns to the native face. |
| `play_motion` | `motion`, optional `duration_ms`, `intensity`, `loop` | Replace the current motion. `"motion":"stop"` fades the current one out and takes no other fields. |
| `blink` | none | Blink once now. |
| `get_state` | none | Current and target expression, kaomoji, motion and its progress, pose, `frames_rendered`, `last_latency_ms`. |
| `list_tags` | none | Tags, presets, eye and mouth shapes, motion names. The debug page builds its buttons from this. |

### app

- Uses the winit 0.30 `ApplicationHandler` with user events (`AppEvent::Api`, `AppEvent::Reload`). The window and surface are created in `resumed`.
- Frame cap: animation runs at most 60 fps on any display (the user's choice, for compatibility with slower devices). `app` never calls `request_redraw` for animation directly. It sets `frame_due = max(now, last_frame + 1/60 s)`, and `about_to_wait` either issues the redraw or sets `WaitUntil(frame_due)`. Only `Resized` and the first frame redraw immediately.
- Idle: `ControlFlow::Wait` by default. While `snapshot.animating`, schedule the next frame through the cap. Otherwise wait until the earliest of `frame_due` and `next_wakeup`. A mouse move triggers a redraw only when the follow target changes. Skip rendering while the window is minimized. Use `PowerPreference::None` and `MemoryHints::MemoryUsage` (the default `Performance` added about 200 MB on the Intel iGPU). With `None`, wgpu takes DXGI adapter 0, the GPU that drives the primary display, so frames never cross adapters. On a normal laptop that is the iGPU; on the dev machine it is the NVIDIA GPU (user's choice, 2026-09-30).
- Global mouse follow: `listen_device_events(DeviceEvents::Always)` makes winit register raw input with `RIDEV_INPUTSINK`, so raw mouse motion arrives even when the window is in the background. Each motion event (and `CursorMoved`, `Moved`, `Resized`, startup) reads the absolute position with `GetCursorPos`, subtracts `inner_position()`, and hands the result to `state`. No polling timer: a still mouse produces no events. Raw deltas are only a wake-up signal because they skip pointer acceleration.
- While the mouse moves anywhere on screen the ball animates. Measured on the dev machine (240 Hz display): 5–14% of one core uncapped, 4–9% with the 60 fps cap (noisy). Idle is still 0.
- Keep the console subsystem (no `windows_subsystem = "windows"`) so the log is visible.

## Data files

`--data <dir>` defaults to `./data`. If a file is missing at startup, use the built-in copy (`include_str!` of the repo's `data/`, parsed in `app`) and log a warning. Until step 4, `app` always uses the built-in copy. The reload thread keeps watching either way. A file that is invalid, or that references an unknown preset or motion, is rejected with a Chinese error (including serde's line and column), and the old table stays live.

```json
// presets.json: every expression preset is applied on top of "neutral", so a tag looks the same every time
{
  "expressions": {
    "neutral": { "eyes": "dot", "mouth": { "shape": "line", "width": 0.35, "open": 0.0, "curve": 0.2 },
                 "blush": 0, "tears": 0, "sweat": 0, "gloom": 0 },
    "smile":   { "eyes": "smile", "mouth": { "shape": "line", "width": 0.45, "open": 0.3, "curve": 0.8 } },
    "scared":  { "eyes": "wide", "mouth": { "shape": "wavy", "width": 0.3 }, "sweat": 1 }
  },
  "motions": { "nod": { "duration_ms": 600, "intensity": 1.0 }, "tremble": { "duration_ms": 1200, "intensity": 1.0 } }
}
// tags.json: expression and motion are both optional; motion fields default from presets.motions
{
  "happy":  { "expression": "smile",  "motion": { "name": "bounce" } },
  "no":     { "motion": { "name": "shake" } },
  "scared": { "expression": "scared", "motion": { "name": "tremble", "loop": true } }
}
```

## Verification

- `cargo test` covers:
  - state: easing, discrete swaps under a blink, motion curves that start and end at 0, loop replacement, tag and preset resolution, follow smoothing.
  - api: HTTP parsing and JSON → Command errors.
  - fonts: a corpus of 30+ kaomoji plus a few common Chinese words all shape with no glyph 0, and `•̀` keeps its base and mark in one font.
- Latency: `api` stamps an `Instant` when a request arrives. `app` logs the time from command to first frame and stores it as `last_latency_ms`. It must stay under 100 ms.
- Idle: when nothing is commanded and the mouse is still, `frames_rendered` in `get_state` must not grow between blinks. Task Manager should show about 0% CPU and GPU.
- Visual: after each milestone, give the user one `cargo run` line and what to click in the debug page. Only the user can confirm the look.
- Screenshots for your own checks go in `_Temp\Mistletoe\gallery`. The comparison overview has 20 tiles in a fixed order (12 presets, blink 0.6 and 1.0, wide blink 0.8, smile blink 1.0, cat flat 0.6, all overlays at 0.5, shy and gloomy turned); keep that layout so the user can compare tile by tile. Never move the user's real cursor or fight their foreground window; they may be using the machine. Use a temporary, uncommitted env-var hook that applies commands at startup and disables cursor follow, and capture with `PrintWindow(hwnd, hdc, PW_CLIENTONLY | PW_RENDERFULLCONTENT)`. Under PowerShell 7, System.Drawing needs extra assembly references, so pixel-analysis scripts run under `powershell.exe` (5.1).

## Build order

1. Scaffold + ball: the Cargo project, the four modules, window, fullscreen triangle, sphere, outline, shading, mouse follow, idle loop.
2. The full `state` module with unit tests: expressions, transitions, blinks, motions, tags and presets.
3. Face rendering in `ball.wgsl`, driven by the Snapshot: eyes, mouth, blush, tears, sweat, gloom.
4. The API, debug page, hot reload, logging, and a README with one curl example per command.
5. Kaomoji: fetch and subset the fonts, shaping with cluster fallback, rasterization, texture, coverage tests. Then an acceptance pass against "Done when".

## Gotchas

- Git for Windows puts its own `link.exe` on PATH. rustc still finds MSVC's linker through VS Build Tools. A `link: extra operand` style error means the wrong `link.exe` was picked up.
- PowerShell 7 passes `'{"cmd":"blink"}'` to `curl.exe` correctly; Windows PowerShell 5.1 does not. The README uses bash syntax and says so.
- Never alpha-blend new tones into the image. Every transition keeps pixels on the palette.
- WGSL errors only surface at runtime: `create_shader_module` panics at startup (for example `set` is a reserved word). After any shader edit, launch the app once, not just `cargo build`.
- Cross-adapter present artifacts: the dev machine's display is on the NVIDIA GPU, so rendering on the Intel iGPU (`LowPower`) makes DXGI copy every frame across adapters. That produced occasional 1-2 px dashed lines across the window (6 of 40 captures); rendering on NVIDIA gave 0 of 40 (2026-09-30). Fixed by `PowerPreference::None` (see app).
- The dev machine has an Intel iGPU plus an RTX 5090 Laptop GPU. wgpu's DX12 adapter enumeration loads both vendors' drivers, so about 330 MB private memory at idle is driver baseline, not a leak (measured 2026-09-30, release build). Every process on this machine shows as `C+G` in `nvidia-smi`, so that listing is not evidence that Mistletoe wakes the dGPU.
