//! The Nuke Merge operations (the `Nuke…` blend modes): PhotoCraft VFX's own code, kept out of
//! `blend.rs` so upstream PhotoCraft's file changes as little as possible and merges stay easy.
//! `blend.rs` reaches this module through a few one-line hooks (`nuke::label`, `nuke::psd_key`,
//! `nuke_channel`, `nuke_composite`); see `FORK.md` at the repository root.
//!
//! Nuke's 32-bit maths, A = source, B = backdrop. Every formula here was measured in Nuke 17
//! (`scripts/nuke_merge_truth.py`: Constant → Premult → Merge2 over colours -0.25, 0.2, 0.8, 2 and
//! alphas 0, 0.3, 1), not taken from the documentation, which is wrong in places: screen, divide,
//! exclusion and geometric switch on negative inputs, and overlay, hard-light, soft-light,
//! color-dodge and color-burn clamp theirs.

use super::BlendMode;
use std::sync::atomic::{AtomicBool, Ordering};

/// Whether blend menus list the Nuke Merge operations ([`BlendMode::NUKE_MODES`]) after
/// Photoshop's modes. The Layers panel's "Nuke blend modes" switch; off by default.
static NUKE_MENU: AtomicBool = AtomicBool::new(false);

/// Show (or hide) the Nuke Merge operations in blend menus ([`BlendMode::menu_modes`]).
pub fn set_nuke_modes_in_menus(on: bool) {
    NUKE_MENU.store(on, Ordering::Relaxed);
}

/// Whether blend menus list the Nuke Merge operations.
pub fn nuke_modes_in_menus() -> bool {
    NUKE_MENU.load(Ordering::Relaxed)
}

impl BlendMode {
    /// Every mode a layer can have but Pass Through: [`Self::layer_modes`], then the Nuke Merge
    /// operations ([`Self::NUKE_MODES`], [`Self::NUKE_HIDDEN_MODES`]).
    pub fn all_modes() -> impl Iterator<Item = Self> {
        Self::layer_modes().chain(Self::NUKE_MODES).chain(Self::NUKE_HIDDEN_MODES)
    }

    /// The modes blend menus list, in menu order: Photoshop's ([`Self::LAYER_MODES`]), then the
    /// Nuke Merge colour operations ([`Self::NUKE_MODES`]) when they are turned on.
    pub fn menu_modes() -> impl Iterator<Item = Self> {
        let nuke: &'static [BlendMode] = if nuke_modes_in_menus() { &Self::NUKE_MODES } else { &[] };
        Self::LAYER_MODES.into_iter().chain(nuke.iter().copied())
    }

    /// Whether blend menus list this mode ([`Self::menu_modes`]).
    pub fn is_menu_mode(self) -> bool {
        Self::LAYER_MODES.contains(&self) || (nuke_modes_in_menus() && Self::NUKE_MODES.contains(&self))
    }

    /// One of the Nuke Merge operations (a `Nuke…` mode).
    pub fn is_nuke(self) -> bool {
        Self::NUKE_MODES.contains(&self) || Self::NUKE_HIDDEN_MODES.contains(&self)
    }

    /// Kept for older call sites: a mode that is not a Nuke Merge operation (or Normal, `over`).
    pub fn is_photoshop_only(self) -> bool {
        self.nuke_op().is_none() && self != BlendMode::PassThrough
    }

    /// The Nuke Merge colour operations blend menus list when they are turned on, grouped like
    /// Photoshop's menu (darken, lighten, contrast, comparative, Nuke-only). `over` is Normal.
    pub const NUKE_MODES: [BlendMode; 18] = [
        BlendMode::NukeMin,
        BlendMode::NukeMultiply,
        BlendMode::NukeColorBurn,
        BlendMode::NukeMax,
        BlendMode::NukeScreen,
        BlendMode::NukeColorDodge,
        BlendMode::NukePlus,
        BlendMode::NukeOverlay,
        BlendMode::NukeSoftLight,
        BlendMode::NukeHardLight,
        BlendMode::NukeDifference,
        BlendMode::NukeExclusion,
        BlendMode::NukeFrom,
        BlendMode::NukeMinus,
        BlendMode::NukeDivide,
        BlendMode::NukeAverage,
        BlendMode::NukeGeometric,
        BlendMode::NukeHypot,
    ];

    /// Nuke Merge operations that are compositing (Porter-Duff / matte) ops rather than colour
    /// blends: in a layer stack B is everything below, so they act on the whole stack (mask,
    /// stencil, in, out, xor), reorder it (under), duplicate clipping masks (atop) or equal over
    /// for straight-alpha layers (matte, conjoint-over, disjoint-over). They still render, so
    /// scripts and documents that use them work, but menus list one only on a layer that has it.
    pub const NUKE_HIDDEN_MODES: [BlendMode; 10] = [
        BlendMode::NukeAtop,
        BlendMode::NukeConjointOver,
        BlendMode::NukeDisjointOver,
        BlendMode::NukeIn,
        BlendMode::NukeMask,
        BlendMode::NukeMatte,
        BlendMode::NukeOut,
        BlendMode::NukeStencil,
        BlendMode::NukeUnder,
        BlendMode::NukeXor,
    ];

    /// Kept for older call sites: [`Self::NUKE_HIDDEN_MODES`].
    pub const HIDDEN_MODES: [BlendMode; 10] = Self::NUKE_HIDDEN_MODES;

    /// The Nuke Merge2 `operation` this mode is: Normal / Pass Through are `over`, a `Nuke…` mode
    /// its own operation; `None` for the Photoshop and Paint.NET modes.
    pub fn nuke_op(self) -> Option<&'static str> {
        Some(match self {
            BlendMode::Normal | BlendMode::PassThrough => "over",
            BlendMode::NukeMin => "min",
            BlendMode::NukeMultiply => "multiply",
            BlendMode::NukeColorBurn => "color-burn",
            BlendMode::NukeUnder => "under",
            BlendMode::NukeIn => "in",
            BlendMode::NukeMax => "max",
            BlendMode::NukeScreen => "screen",
            BlendMode::NukeColorDodge => "color-dodge",
            BlendMode::NukePlus => "plus",
            BlendMode::NukeOut => "out",
            BlendMode::NukeOverlay => "overlay",
            BlendMode::NukeSoftLight => "soft-light",
            BlendMode::NukeHardLight => "hard-light",
            BlendMode::NukeGeometric => "geometric",
            BlendMode::NukeHypot => "hypot",
            BlendMode::NukeMask => "mask",
            BlendMode::NukeStencil => "stencil",
            BlendMode::NukeDifference => "difference",
            BlendMode::NukeExclusion => "exclusion",
            BlendMode::NukeMinus => "minus",
            BlendMode::NukeDivide => "divide",
            BlendMode::NukeMatte => "matte",
            BlendMode::NukeAverage => "average",
            BlendMode::NukeConjointOver => "conjoint-over",
            BlendMode::NukeDisjointOver => "disjoint-over",
            BlendMode::NukeXor => "xor",
            BlendMode::NukeFrom => "from",
            BlendMode::NukeAtop => "atop",
            _ => return None,
        })
    }

    /// The Nuke Merge operation closest to a Photoshop mode, for scripts written for Nuke:
    /// (operation, exact). `over` (inexact) where Nuke has nothing close.
    pub fn nearest_nuke_op(self) -> (&'static str, bool) {
        if let Some(op) = self.nuke_op() {
            return (op, true);
        }
        let op = match self {
            BlendMode::Darken => "min",
            BlendMode::Multiply => "multiply",
            BlendMode::ColorBurn => "color-burn",
            BlendMode::Lighten => "max",
            BlendMode::Screen => "screen",
            BlendMode::ColorDodge => "color-dodge",
            BlendMode::LinearDodge => "plus",
            BlendMode::Overlay => "overlay",
            BlendMode::SoftLight => "soft-light",
            BlendMode::HardLight => "hard-light",
            BlendMode::Difference => "difference",
            BlendMode::Exclusion => "exclusion",
            BlendMode::Subtract => "from",
            _ => "over",
        };
        (op, false)
    }
}

/// [`BlendMode::label`] of a Nuke Merge operation; empty for any other mode.
pub(super) fn label(mode: BlendMode) -> &'static str {
    match mode {
        BlendMode::NukeMin => "min (Nuke)",
        BlendMode::NukeMultiply => "multiply (Nuke)",
        BlendMode::NukeColorBurn => "color-burn (Nuke)",
        BlendMode::NukeUnder => "under (Nuke)",
        BlendMode::NukeIn => "in (Nuke)",
        BlendMode::NukeMax => "max (Nuke)",
        BlendMode::NukeScreen => "screen (Nuke)",
        BlendMode::NukeColorDodge => "color-dodge (Nuke)",
        BlendMode::NukePlus => "plus (Nuke)",
        BlendMode::NukeOut => "out (Nuke)",
        BlendMode::NukeOverlay => "overlay (Nuke)",
        BlendMode::NukeSoftLight => "soft-light (Nuke)",
        BlendMode::NukeHardLight => "hard-light (Nuke)",
        BlendMode::NukeGeometric => "geometric (Nuke)",
        BlendMode::NukeHypot => "hypot (Nuke)",
        BlendMode::NukeMask => "mask (Nuke)",
        BlendMode::NukeStencil => "stencil (Nuke)",
        BlendMode::NukeDifference => "difference (Nuke)",
        BlendMode::NukeExclusion => "exclusion (Nuke)",
        BlendMode::NukeMinus => "minus (Nuke)",
        BlendMode::NukeDivide => "divide (Nuke)",
        BlendMode::NukeMatte => "matte (Nuke)",
        BlendMode::NukeAverage => "average (Nuke)",
        BlendMode::NukeConjointOver => "conjoint-over (Nuke)",
        BlendMode::NukeDisjointOver => "disjoint-over (Nuke)",
        BlendMode::NukeXor => "xor (Nuke)",
        BlendMode::NukeFrom => "from (Nuke)",
        BlendMode::NukeAtop => "atop (Nuke)",
        _ => "",
    }
}

/// [`BlendMode::psd_key`] of a Nuke Merge operation: the colour operations are written as the
/// Photoshop mode closest to them, the rest (and any other mode) as Normal.
pub(super) fn psd_key(mode: BlendMode) -> [u8; 4] {
    *match mode {
        BlendMode::NukeMin => b"dark",
        BlendMode::NukeMultiply => b"mul ",
        BlendMode::NukeColorBurn => b"idiv",
        BlendMode::NukeMax => b"lite",
        BlendMode::NukeScreen => b"scrn",
        BlendMode::NukeColorDodge => b"div ",
        BlendMode::NukePlus => b"lddg",
        BlendMode::NukeOverlay => b"over",
        BlendMode::NukeSoftLight => b"sLit",
        BlendMode::NukeHardLight => b"hLit",
        BlendMode::NukeDifference => b"diff",
        BlendMode::NukeExclusion => b"smud",
        BlendMode::NukeFrom => b"fsub",
        _ => b"norm",
    }
}

#[inline]
/// Nuke's screen: `A + B - AB` where both are within 0..1, else `max(A, B)`. Light above 1 and
/// negative ACEScg values both take the max.
pub fn nuke_screen(a: f32, b: f32) -> f32 {
    if (0.0..=1.0).contains(&a) && (0.0..=1.0).contains(&b) { a + b - a * b } else { a.max(b) }
}

/// Nuke's multiply: `AB`, or `A` where both are negative, which stops two negatives making a
/// positive.
#[inline]
pub fn nuke_multiply(a: f32, b: f32) -> f32 {
    if a < 0.0 && b < 0.0 { a } else { a * b }
}

/// Shared core of Nuke's `overlay` and `hard-light`, `x` the deciding input: `2xy` where
/// `x ≤ 0.5`, else `1 − 2(1−x)(1−y)`. Nuke clamps `x` to 0..1 and `y` to 0 and above, and the
/// result to 1 and below: these two are the display-range operations of Merge.
#[inline]
pub fn nuke_light(x: f32, y: f32) -> f32 {
    let (x, y) = (x.clamp(0.0, 1.0), y.max(0.0));
    let v = if x <= 0.5 { 2.0 * x * y } else { 1.0 - 2.0 * (1.0 - x) * (1.0 - y) };
    v.min(1.0)
}

/// Nuke `overlay` (A = `a`, B = `b`): multiply if B < 0.5, screen if B > 0.5.
#[inline]
pub fn nuke_overlay(a: f32, b: f32) -> f32 {
    nuke_light(b, a)
}

/// Nuke `hard-light` (A = `a`, B = `b`): multiply if A < 0.5, screen if A > 0.5.
#[inline]
pub fn nuke_hard_light(a: f32, b: f32) -> f32 {
    nuke_light(a, b)
}

/// Nuke's soft-light core: `B(2A + B(1 − p))` where `p < 1`, else `2AB`, inputs clamped to 0 and
/// above. Unmasked, Nuke runs it on the premultiplied values with `p = AB`; with alpha masking,
/// on the straight colours with `p` = straight A times premultiplied B ([`nuke_composite`]).
#[inline]
pub(super) fn nuke_soft_light_p(a: f32, b: f32, p: f32) -> f32 {
    if p < 1.0 { b * (2.0 * a + b * (1.0 - p)) } else { 2.0 * a * b }
}

/// Nuke's soft-light on one pair of values (A = `a`, B = `b`), as an unmasked Merge applies it.
#[inline]
pub fn nuke_soft_light(a: f32, b: f32) -> f32 {
    let (a, b) = (a.max(0.0), b.max(0.0));
    nuke_soft_light_p(a, b, a * b)
}

#[inline]
/// Nuke's divide: `A / B` where B is positive, else `A` (B at or below 0 leaves A as it is).
pub fn nuke_divide(a: f32, b: f32) -> f32 {
    if b > 0.0 { a / b } else { a }
}

/// Nuke's exclusion: `A + B − 2AB` where both are at least 0, else `A + B`.
#[inline]
pub fn nuke_exclusion(a: f32, b: f32) -> f32 {
    if a < 0.0 || b < 0.0 { a + b } else { a + b - 2.0 * a * b }
}

/// Nuke's geometric: `2AB / (A + B)`, or 0 where either is negative (or both are 0).
#[inline]
pub fn nuke_geometric(a: f32, b: f32) -> f32 {
    let sum = a + b;
    if a < 0.0 || b < 0.0 || sum == 0.0 { 0.0 } else { 2.0 * a * b / sum }
}

/// Nuke's color-dodge: `B / (1 − A)`, inputs clamped to 0 and above and the result to 1 and below
/// (1 where A ≥ 1).
#[inline]
pub fn nuke_color_dodge(a: f32, b: f32) -> f32 {
    let (a, b) = (a.max(0.0), b.max(0.0));
    if a >= 1.0 { 1.0 } else { (b / (1.0 - a)).min(1.0) }
}

/// Nuke's color-burn: `1 − (1 − B) / A`, B clamped to 0 and above and the result to 0 and above;
/// 0 where A ≤ 0.
#[inline]
pub fn nuke_color_burn(a: f32, b: f32) -> f32 {
    if a <= 0.0 { 0.0 } else { (1.0 - (1.0 - b.max(0.0)) / a).max(0.0) }
}

/// The operations Nuke runs on inputs clamped to 0 and above, the comp's colour included.
#[inline]
pub(super) fn nuke_clamps_inputs(mode: BlendMode) -> bool {
    matches!(mode, BlendMode::NukeOverlay | BlendMode::NukeHardLight | BlendMode::NukeSoftLight | BlendMode::NukeColorDodge | BlendMode::NukeColorBurn)
}

/// `B(cb, cs)` of a Nuke colour operation (A = `cs`, B = `cb`); the compositing ones (in, out,
/// mask, stencil, atop, matte, xor, conjoint / disjoint over, under) are composited whole in
/// [`composite`](super::composite), and give the source here.
#[inline]
pub(super) fn nuke_channel(mode: BlendMode, cb: f32, cs: f32) -> f32 {
    match mode {
        BlendMode::NukeUnder | BlendMode::NukeAtop => cb,
        BlendMode::NukePlus => cs + cb,
        BlendMode::NukeMultiply => nuke_multiply(cs, cb),
        BlendMode::NukeScreen => nuke_screen(cs, cb),
        BlendMode::NukeMinus => cs - cb,
        BlendMode::NukeFrom => cb - cs,
        BlendMode::NukeDivide => nuke_divide(cs, cb),
        BlendMode::NukeMin | BlendMode::NukeIn => cs.min(cb),
        BlendMode::NukeMax | BlendMode::NukeOut => cs.max(cb),
        BlendMode::NukeHypot => (cs * cs + cb * cb).sqrt(),
        BlendMode::NukeGeometric => nuke_geometric(cs, cb),
        BlendMode::NukeDifference => (cs - cb).abs(),
        BlendMode::NukeExclusion => nuke_exclusion(cs, cb),
        BlendMode::NukeOverlay => nuke_overlay(cs, cb),
        BlendMode::NukeHardLight => nuke_hard_light(cs, cb),
        BlendMode::NukeSoftLight => nuke_soft_light(cs, cb),
        BlendMode::NukeAverage => 0.5 * (cs + cb),
        BlendMode::NukeColorDodge => nuke_color_dodge(cs, cb),
        BlendMode::NukeColorBurn => nuke_color_burn(cs, cb),
        _ => cs,
    }
}

/// [`composite`](super::composite) for the Nuke Merge operations: Nuke's premultiplied maths, then mixed by
/// `opacity` (Merge's `mix`). The compositing operations are composited whole here; the colour
/// ones use the general formula with [`nuke_channel`].
pub(super) fn nuke_composite(mode: BlendMode, backdrop: [f32; 4], source: [f32; 4], opacity: f32) -> [f32; 4] {
    let mix = opacity.clamp(0.0, 1.0);
    if mix <= 0.0 {
        return backdrop;
    }

    let ba = backdrop[3];
    let b = [backdrop[0] * ba, backdrop[1] * ba, backdrop[2] * ba, ba];

    let sa = source[3];
    let a = [source[0] * sa, source[1] * sa, source[2] * sa, sa];

    // An empty source leaves the backdrop as it was for most operations. Not for these: in, mask
    // and out give nothing there (Ab, Ba, A(1-b) with A or a zero), and min, max and hypot treat
    // the empty A as a value, which differs from B where B is negative.
    if sa <= 0.0
        && !matches!(mode, BlendMode::NukeIn | BlendMode::NukeMask | BlendMode::NukeOut | BlendMode::NukeMin | BlendMode::NukeMax | BlendMode::NukeHypot)
    {
        return backdrop;
    }

    let m: [f32; 4] = match mode {
        BlendMode::NukeMatte => [
            // matte
            a[0] + b[0] * (1.0 - sa),
            a[1] + b[1] * (1.0 - sa),
            a[2] + b[2] * (1.0 - sa),
            sa + ba * (1.0 - sa),
        ],
        BlendMode::NukeUnder => [
            // under
            a[0] * (1.0 - ba) + b[0],
            a[1] * (1.0 - ba) + b[1],
            a[2] * (1.0 - ba) + b[2],
            sa * (1.0 - ba) + ba,
        ],
        BlendMode::NukePlus => [
            // plus
            a[0] + b[0],
            a[1] + b[1],
            a[2] + b[2],
            (sa + ba).min(1.0),
        ],
        BlendMode::NukeScreen => [
            // screen
            nuke_screen(a[0], b[0]),
            nuke_screen(a[1], b[1]),
            nuke_screen(a[2], b[2]),
            nuke_screen(sa, ba),
        ],
        BlendMode::NukeMin => [
            // min
            a[0].min(b[0]),
            a[1].min(b[1]),
            a[2].min(b[2]),
            sa.min(ba),
        ],
        BlendMode::NukeMax => [
            // max
            a[0].max(b[0]),
            a[1].max(b[1]),
            a[2].max(b[2]),
            sa.max(ba),
        ],
        BlendMode::NukeHypot => [
            // hypot
            (a[0] * a[0] + b[0] * b[0]).sqrt(),
            (a[1] * a[1] + b[1] * b[1]).sqrt(),
            (a[2] * a[2] + b[2] * b[2]).sqrt(),
            (sa * sa + ba * ba).sqrt().min(1.0),
        ],
        BlendMode::NukeIn => [a[0] * ba, a[1] * ba, a[2] * ba, sa * ba], // in
        BlendMode::NukeOut => [
            // out
            a[0] * (1.0 - ba),
            a[1] * (1.0 - ba),
            a[2] * (1.0 - ba),
            sa * (1.0 - ba),
        ],
        BlendMode::NukeMask => [b[0] * sa, b[1] * sa, b[2] * sa, ba * sa], // mask
        BlendMode::NukeStencil => [
            // stencil
            b[0] * (1.0 - sa),
            b[1] * (1.0 - sa),
            b[2] * (1.0 - sa),
            ba * (1.0 - sa),
        ],
        BlendMode::NukeAtop => [
            // atop
            a[0] * ba + b[0] * (1.0 - sa),
            a[1] * ba + b[1] * (1.0 - sa),
            a[2] * ba + b[2] * (1.0 - sa),
            ba,
        ],
        BlendMode::NukeXor => [
            // xor
            a[0] * (1.0 - ba) + b[0] * (1.0 - sa),
            a[1] * (1.0 - ba) + b[1] * (1.0 - sa),
            a[2] * (1.0 - ba) + b[2] * (1.0 - sa),
            (sa * (1.0 - ba) + ba * (1.0 - sa)).clamp(0.0, 1.0),
        ],
        BlendMode::NukeConjointOver => {
            // conjoint-over
            if sa > ba || ba <= 1e-7 {
                a
            } else {
                let k = 1.0 - sa / ba;
                [a[0] + b[0] * k, a[1] + b[1] * k, a[2] + b[2] * k, sa + ba * k]
            }
        }
        BlendMode::NukeDisjointOver => {
            // disjoint-over
            if sa + ba < 1.0 || ba <= 1e-7 {
                [a[0] + b[0], a[1] + b[1], a[2] + b[2], (sa + ba).min(1.0)]
            } else {
                let k = (1.0 - sa) / ba;
                [a[0] + b[0] * k, a[1] + b[1] * k, a[2] + b[2] * k, 1.0]
            }
        }
        // Every other operation runs as the export writes it, with Merge's alpha masking
        // (`screen_alpha`): the alpha is a + b - ab, each input passes through where the other
        // doesn't cover, `(1-b)A + (1-a)B` premultiplied, and the operation adds its share where
        // both do. Nuke's share differs by operation (measured in Nuke 17): most take `ab·f` of
        // the straight colours, minus and from `ab·f` of the premultiplied ones, divide and
        // geometric `f` of the premultiplied ones, and the clamping ones clamp both colours first,
        // in the pass-through terms too. Applying the formula to the alpha instead, as a plain
        // Merge does, gives difference, minus and from an alpha of 0 over an opaque comp.
        other => {
            let clamp = nuke_clamps_inputs(other);
            let (cs, cb) = if clamp {
                ([source[0].max(0.0), source[1].max(0.0), source[2].max(0.0)], [backdrop[0].max(0.0), backdrop[1].max(0.0), backdrop[2].max(0.0)])
            } else {
                ([source[0], source[1], source[2]], [backdrop[0], backdrop[1], backdrop[2]])
            };
            let (ap, bp) = ([cs[0] * sa, cs[1] * sa, cs[2] * sa], [cb[0] * ba, cb[1] * ba, cb[2] * ba]);
            let both = sa * ba;
            let share = |i: usize| -> f32 {
                match other {
                    BlendMode::NukeMinus => both * (ap[i] - bp[i]),
                    BlendMode::NukeFrom => both * (bp[i] - ap[i]),
                    BlendMode::NukeDivide => {
                        if bp[i] > 0.0 {
                            ap[i] / bp[i]
                        } else {
                            both * cs[i]
                        }
                    }
                    BlendMode::NukeGeometric => nuke_geometric(ap[i], bp[i]),
                    BlendMode::NukeSoftLight => both * nuke_soft_light_p(cs[i], cb[i], cs[i] * bp[i]),
                    _ => both * nuke_channel(other, cb[i], cs[i]),
                }
            };
            let ao = sa + ba * (1.0 - sa);
            [
                (1.0 - sa) * bp[0] + (1.0 - ba) * ap[0] + share(0),
                (1.0 - sa) * bp[1] + (1.0 - ba) * ap[1] + share(1),
                (1.0 - sa) * bp[2] + (1.0 - ba) * ap[2] + share(2),
                ao,
            ]
        }
    };

    let out_pm = [b[0] + (m[0] - b[0]) * mix, b[1] + (m[1] - b[1]) * mix, b[2] + (m[2] - b[2]) * mix, (b[3] + (m[3] - b[3]) * mix).clamp(0.0, 1.0)];

    let ao = out_pm[3];
    if ao <= 1e-7 {
        [0.0; 4]
    } else {
        let inv_a = 1.0 / ao;
        [out_pm[0] * inv_a, out_pm[1] * inv_a, out_pm[2] * inv_a, ao]
    }
}
