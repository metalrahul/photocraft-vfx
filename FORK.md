# PhotoCraft VFX: fork notes

PhotoCraft VFX is a fork of [storytold/photocraft](https://github.com/storytold/photocraft). This
file is the checklist for merging upstream: what the fork adds, where it hooks into upstream's
files, and how to merge.

## The rule: own files, one-line hooks

Fork code goes in files upstream doesn't have. Where upstream's code has to call into it, the
upstream file gets a hook of a line or two, listed below. A merge can then only conflict on those
hook lines, and the conflict is easy to read: keep upstream's change and put the hook back.

When you add a fork feature, follow the same pattern and add its hooks to the table.

## Hooks into upstream files

### Blend modes: `crates/color/src/blend.rs` → `crates/color/src/blend/nuke.rs`

`nuke.rs` holds the Nuke Merge operations: the menu switch, the `NUKE_MODES` and
`NUKE_HIDDEN_MODES` lists, `nuke_op`, labels, PSD keys, Nuke's maths and `nuke_composite`.
Tests: `blend/nuke_tests.rs`, checked against Nuke 17 (`blend/nuke17_merge.csv`).

| In `blend.rs` | Hook |
|---|---|
| `enum BlendMode` | the `Nuke…` variants, at the end of the enum |
| `enum BlendMode` | `#[serde(alias = "Ps…")]` on nine Photoshop modes (old fork documents) |
| `has_psd_equivalent` | `&& !self.is_nuke()` |
| `label` | last arm `_ => nuke::label(self)` |
| `psd_key` | last arm `_ => return nuke::psd_key(self)` |
| `channel` | last arm `_ => nuke_channel(mode, cb, cs)` |
| end of file | `pub mod nuke;`, the `pub use nuke::{…}` re-exports, `mod nuke_tests;` |

Not Nuke, but also fork code in `blend.rs`: `range_bound` and its check at the top of `channel`
(32-bit Photoshop modes past 0..1), and `composite_hdr` / `composite_for` / `composite_with_blend`
and their `atop` versions (32-bit compositing). These are candidates for the next move.

### Compositor shader: `crates/gpu/src/compose.wgsl` → `crates/gpu/src/vfx.wgsl`

`vfx.wgsl` holds the Nuke operations (`NUKE_BASE`, `nuke_rgb`, `nuke_masked_share`,
`nuke_composite`), the exact 32-bit Color Burn / Dodge / Vivid Light and `range_bound`, and the
Grade and ColorLookup kernels. `kit::SHADER` compiles it after `compose.wgsl` as one module (WGSL
declarations can come in any order, the same way upstream joins its Camera Raw shaders).

| Where | Hook |
|---|---|
| `kit.rs` `SHADER` | `concat!(include_str!("compose.wgsl"), "\n", include_str!("vfx.wgsl"))` |
| `compose.wgsl` `blend_channel` | `hdr` flag, the `range_bound` branch around the switch, and `hdr` choices in cases 4, 5, 10, 16 |
| `compose.wgsl` `blend_rgb` | `if (mode >= NUKE_BASE) { return nuke_rgb(…); }` |
| `compose.wgsl` `composite` | `if (mode >= NUKE_BASE) { return nuke_composite(…); }` |
| `compose.wgsl` `composite_fill` | `if (mode >= NUKE_BASE && !gamma_on) { … }` |
| `compose.wgsl` `adjust` | `case 18` (Grade) and `case 19` (ColorLookup Curves) |

Keep the shader's DX12 rules when editing either file: in a `switch`, every case assigns and the
function returns once at the end, and the Nuke maths stay on whole `vec3`s with `select` (FXC
fails on per-channel loops).

### Still spread through upstream files

Other fork features still edit upstream files directly. These are the next ones to move out, in
order of how often they conflicted:

- `crates/compose/src/psblend.rs`: 32-bit blending and Lab mixing
- `crates/ui-egui/src/canvas/strokes.rs`: Large Brush Mode
- `crates/ui-egui/src/dialogs/adjust_editors/curves.rs`: the Curves editor's range and hold-ends controls
- `crates/ui-egui/src/menus/bar.rs` and `panels/status_bar.rs`: OCIO menu items and the viewer bar
- `crates/compose/src/adjust.rs` and `adjust/tone.rs`: Grade, ColorLookup Curves and `spline_knots`

Files that are only the fork's (no conflicts possible): `crates/ocio/`, `crates/io/src/exr_export*.rs`,
`crates/engine/src/ocio_viewer.rs`, `preview_display.rs`, `crates/gpu/src/live.rs`,
`scripts/nuke_*_truth.py`, and the other new files.

## Merging upstream (about once a month)

One-time setup on your machine:

```sh
git remote add upstream https://github.com/storytold/photocraft.git
git config rerere.enabled true   # git remembers how you resolved a conflict and reapplies it
```

Each merge:

```sh
git fetch upstream
git switch vfx
git switch -c merge/upstream-<date>
git merge upstream/main
```

1. For each conflict, check the file against the hook tables above. Keep upstream's code and put
   the hook back.
2. If upstream added a blend mode, add it to `LAYER_MODES` as upstream did, and check
   `gpu::plan::lower::mode_index` still puts the Nuke modes from `NUKE_BASE` (34) on.
3. Build and test: `cargo test -p photocraft-color -p photocraft-compose -p photocraft-gpu
   -p photocraft-io -p photocraft-engine`. The Nuke tests compare against Nuke 17's numbers, so a
   failure there means a hook was lost.
4. Open the app and check one 32-bit document with a Nuke blend mode, Grade and Curves.
5. Merge the branch into `vfx` and `main`.

Known differences from upstream's tests: upstream's
`hdr_screen_replays_only_nonfinite_results_in_wide_arithmetic` and
`overflowing_hdr_retries_keep_signed_results_and_blend_identities` expect Photoshop's 0..1 formulas
past 1 in 32-bit documents, where the fork's `range_bound` keeps the brighter input instead. They
fail by design until we decide which behaviour to keep.
