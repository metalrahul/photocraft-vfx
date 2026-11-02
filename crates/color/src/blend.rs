//! Nuke-style 32-bit floating-point linear Merge modes (scalar reference implementation).

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum BlendMode {
    PassThrough,
    Normal,
    Dissolve,
    Darken,
    Multiply,
    ColorBurn,
    LinearBurn,
    DarkerColor,
    Lighten,
    Screen,
    ColorDodge,
    LinearDodge,
    LighterColor,
    Overlay,
    SoftLight,
    HardLight,
    VividLight,
    LinearLight,
    PinLight,
    HardMix,
    Difference,
    Exclusion,
    Subtract,
    Divide,
    Hue,
    Saturation,
    Color,
    Luminosity,
    Reflect,
    Glow,
    Negation,
    Xor,
    PaintNetColorBurn,
    PaintNetColorDodge,
}

impl BlendMode {
    /// Nuke Merge operations that are compositing (Porter-Duff / matte) ops rather than colour
    /// blends, or that duplicate a layer-editor feature: in a layer stack B is everything below,
    /// so they act on the whole stack (mask, stencil, in, out, xor, copy), reorder it (under),
    /// duplicate clipping masks (atop) or equal `over` for straight-alpha layers (matte,
    /// conjoint-over, disjoint-over). They stay fully supported, so documents and scripts that use
    /// them still load and render, but blend menus only list one when a layer already has it.
    /// Saturation / Color / Luminosity have no Nuke Merge counterpart.
    pub const HIDDEN_MODES: [BlendMode; 14] = [
        BlendMode::PaintNetColorDodge, // atop
        BlendMode::Glow,               // conjoint-over
        BlendMode::Dissolve,           // copy
        BlendMode::Negation,           // disjoint-over
        BlendMode::DarkerColor,        // in
        BlendMode::PinLight,           // mask
        BlendMode::Hue,                // matte
        BlendMode::LighterColor,       // out
        BlendMode::HardMix,            // stencil
        BlendMode::LinearBurn,         // under
        BlendMode::Xor,                // xor
        BlendMode::Saturation,
        BlendMode::Color,
        BlendMode::Luminosity,
    ];

    /// Kept for older call sites (it used to hold the Paint.NET-only modes): the modes that blend
    /// menus hide unless a layer already uses one. See [`Self::HIDDEN_MODES`].
    pub const PAINT_NET_MODES: [BlendMode; 14] = Self::HIDDEN_MODES;

    /// Every mode a layer can have: the menu modes, then the hidden ones.
    pub fn layer_modes() -> impl Iterator<Item = Self> {
        Self::LAYER_MODES.into_iter().chain(Self::HIDDEN_MODES)
    }

    /// Whether blend menus list this mode ([`Self::LAYER_MODES`]).
    pub fn is_menu_mode(self) -> bool {
        Self::LAYER_MODES.contains(&self)
    }

    /// Whether Photoshop has a mode with the same maths on 0..1 values, so a PSD keeps it.
    /// Soft Light counts (closest match; Nuke's curve differs slightly from Photoshop's).
    /// `minus` (A − B), Nuke's `divide` (A / B; Photoshop divides B / A), average, geometric,
    /// hypot and the hidden compositing ops have none: PSD export writes them as Normal and warns.
    pub fn has_psd_equivalent(self) -> bool {
        matches!(
            self,
            BlendMode::PassThrough
                | BlendMode::Normal
                | BlendMode::Multiply
                | BlendMode::Screen
                | BlendMode::Overlay
                | BlendMode::SoftLight
                | BlendMode::HardLight
                | BlendMode::ColorDodge
                | BlendMode::ColorBurn
                | BlendMode::LinearDodge
                | BlendMode::PaintNetColorBurn
                | BlendMode::Difference
                | BlendMode::Exclusion
                | BlendMode::Darken
                | BlendMode::Lighten
        )
    }

    /// The modes blend menus list: Nuke Merge colour operations that make sense between layers,
    /// grouped like Photoshop's menu (normal, darken, lighten, contrast, comparative, Nuke-only).
    pub const LAYER_MODES: [BlendMode; 19] = [
        BlendMode::Normal,            // over
        BlendMode::Darken,            // min
        BlendMode::Multiply,          // multiply
        BlendMode::ColorBurn,         // color-burn
        BlendMode::Lighten,           // max
        BlendMode::Screen,            // screen
        BlendMode::ColorDodge,        // color-dodge
        BlendMode::LinearDodge,       // plus
        BlendMode::Overlay,           // overlay
        BlendMode::SoftLight,         // soft-light
        BlendMode::HardLight,         // hard-light
        BlendMode::Difference,        // difference
        BlendMode::Exclusion,         // exclusion
        BlendMode::PaintNetColorBurn, // from (B − A, Photoshop's Subtract)
        BlendMode::Subtract,          // minus (A − B)
        BlendMode::Divide,            // divide
        BlendMode::Reflect,           // average
        BlendMode::VividLight,        // geometric
        BlendMode::LinearLight,       // hypot
    ];

    pub fn label(self) -> &'static str {
        match self {
            BlendMode::PassThrough => "pass-through",
            BlendMode::PaintNetColorDodge => "atop",
            BlendMode::Reflect => "average",
            BlendMode::ColorBurn => "color-burn",
            BlendMode::ColorDodge => "color-dodge",
            BlendMode::Glow => "conjoint-over",
            BlendMode::Dissolve => "copy",
            BlendMode::Difference => "difference",
            BlendMode::Negation => "disjoint-over",
            BlendMode::Divide => "divide",
            BlendMode::Exclusion => "exclusion",
            BlendMode::PaintNetColorBurn => "from",
            BlendMode::VividLight => "geometric",
            BlendMode::HardLight => "hard-light",
            BlendMode::LinearLight => "hypot",
            BlendMode::DarkerColor => "in",
            BlendMode::PinLight => "mask",
            BlendMode::Hue => "matte",
            BlendMode::Lighten => "max",
            BlendMode::Darken => "min",
            BlendMode::Subtract => "minus",
            BlendMode::Multiply => "multiply",
            BlendMode::LighterColor => "out",
            BlendMode::Normal => "over",
            BlendMode::Overlay => "overlay",
            BlendMode::LinearDodge => "plus",
            BlendMode::Screen => "screen",
            BlendMode::SoftLight => "soft-light",
            BlendMode::HardMix => "stencil",
            BlendMode::LinearBurn => "under",
            BlendMode::Xor => "xor",
            BlendMode::Saturation => "saturation (unused)",
            BlendMode::Color => "color (unused)",
            BlendMode::Luminosity => "luminosity (unused)",
        }
    }

    /// The Photoshop key this mode is written as: the Photoshop mode with the same maths, or
    /// Normal when there is none ([`Self::has_psd_equivalent`]).
    pub fn psd_key(self) -> [u8; 4] {
        *match self {
            BlendMode::PassThrough => b"pass",
            BlendMode::Normal => b"norm",
            BlendMode::Multiply => b"mul ",
            BlendMode::Screen => b"scrn",
            BlendMode::Overlay => b"over",
            BlendMode::SoftLight => b"sLit",
            BlendMode::HardLight => b"hLit",
            BlendMode::ColorDodge => b"div ",
            BlendMode::ColorBurn => b"idiv",
            BlendMode::LinearDodge => b"lddg",          // plus = Linear Dodge (Add)
            BlendMode::PaintNetColorBurn => b"fsub",    // from (B − A) = Subtract
            BlendMode::Difference => b"diff",
            BlendMode::Exclusion => b"smud",
            BlendMode::Darken => b"dark",               // min
            BlendMode::Lighten => b"lite",              // max
            _ => b"norm",
        }
    }

    /// The mode a Photoshop key reads as. Photoshop modes Nuke has no Merge operation for
    /// (Dissolve, Linear Burn, Vivid / Linear / Pin Light, Hard Mix, Darker / Lighter Color,
    /// Divide, Hue, Saturation, Color, Luminosity) return None: the importer warns and uses over.
    pub fn from_psd_key(key: [u8; 4]) -> Option<BlendMode> {
        std::iter::once(BlendMode::PassThrough)
            .chain(Self::LAYER_MODES)
            .filter(|m| m.has_psd_equivalent())
            .find(|m| m.psd_key() == key)
    }

    pub fn is_separable(self) -> bool {
        !matches!(
            self,
            BlendMode::Hue
                | BlendMode::Saturation
                | BlendMode::Color
                | BlendMode::Luminosity
        )
    }
}

#[inline]
pub fn nuke_screen(a: f32, b: f32) -> f32 {
    if (0.0..=1.0).contains(&a) && (0.0..=1.0).contains(&b) {
        a + b - a * b
    } else {
        a.max(b)
    }
}

/// Shared core of Nuke's `overlay` and `hard-light`: multiply below 0.5 of the deciding input
/// `x`, screen above it, with `x` (not the other input) rescaled to 0..1 in the screen branch:
/// `x < 0.5 → 2·x·y`, else `screen(2·x − 1, y)`. Both branches give `y` at `x = 0.5`, so the
/// result is continuous. Nothing is clamped: 32-bit values above 1 (and below 0) pass through,
/// and [`nuke_screen`]'s `max` fallback handles them as Nuke does.
///
/// The previous version clamped both inputs to 0..1 (flattening every highlight above 1) and
/// rescaled the *other* input in the screen branch, so wherever `x > 0.5` and `y < 0.5` the
/// screen saw a negative operand, fell back to `max` and returned `x` outright: hard edges along
/// the backdrop's 0.5 contour and a much brighter result than Nuke.
#[inline]
pub fn nuke_light(x: f32, y: f32) -> f32 {
    if x < 0.5 {
        2.0 * x * y
    } else {
        nuke_screen(2.0 * x - 1.0, y)
    }
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

#[inline]
pub fn nuke_soft_light(a: f32, b: f32) -> f32 {
    let a_c = a.clamp(0.0, 1.0);
    let b_c = b.clamp(0.0, 1.0);
    if a_c * b_c < 1.0 {
        b_c * (2.0 * a_c + (b_c * (1.0 - a_c * 2.0)))
    } else {
        2.0 * b_c * a_c
    }
}

#[inline]
pub fn nuke_divide(a: f32, b: f32) -> f32 {
    if b.abs() <= 1e-7 {
        0.0
    } else {
        a / b
    }
}

#[inline]
pub fn blend_channel(mode: BlendMode, cb: f32, cs: f32) -> f32 {
    blend_channel_hdr(mode, cb, cs)
}

#[inline]
pub fn blend_channel_hdr(mode: BlendMode, cb: f32, cs: f32) -> f32 {
    match mode {
        BlendMode::Normal | BlendMode::PassThrough => cs, // over
        BlendMode::Dissolve => cs, // copy
        BlendMode::LinearBurn => cb, // under
        BlendMode::LinearDodge => cs + cb, // plus
        BlendMode::Multiply => cs * cb, // multiply
        BlendMode::Screen => nuke_screen(cs, cb), // screen
        BlendMode::Subtract => cs - cb, // minus
        BlendMode::PaintNetColorBurn => cb - cs, // from
        BlendMode::Divide => nuke_divide(cs, cb), // divide
        BlendMode::Darken | BlendMode::DarkerColor => cs.min(cb), // min / in
        BlendMode::Lighten | BlendMode::LighterColor => cs.max(cb), // max / out
        BlendMode::LinearLight => (cs * cs + cb * cb).sqrt(), // hypot
        BlendMode::VividLight => { // geometric
            let sum = cs + cb;
            if sum.abs() <= 1e-7 { 0.0 } else { (2.0 * cs * cb) / sum }
        }
        BlendMode::Difference => (cs - cb).abs(), // difference
        BlendMode::Exclusion => cs + cb - 2.0 * cs * cb, // exclusion
        BlendMode::Overlay => nuke_overlay(cs, cb), // overlay
        BlendMode::HardLight => nuke_hard_light(cs, cb), // hard-light
        BlendMode::SoftLight => nuke_soft_light(cs, cb), // soft-light
        BlendMode::Reflect => 0.5 * (cs + cb), // average
        BlendMode::ColorDodge => { // color-dodge
            let denom = 1.0 - cs;
            if denom.abs() <= 1e-6 { cb } else { cb / denom }
        }
        BlendMode::ColorBurn => { // color-burn
            if cs.abs() <= 1e-6 { 0.0 } else { 1.0 - (1.0 - cb) / cs }
        }
        BlendMode::PaintNetColorDodge => cb, // atop (handled in RGBA composite)
        BlendMode::Hue => cs, // matte (handled in RGBA composite)
        BlendMode::PinLight | BlendMode::HardMix | BlendMode::Glow | BlendMode::Negation | BlendMode::Xor | BlendMode::Saturation | BlendMode::Color | BlendMode::Luminosity => cs,
    }
}

#[inline]
pub fn lum(c: [f32; 3]) -> f32 {
    0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2]
}

fn clip_color(c: [f32; 3], top: f32) -> [f32; 3] {
    let l = lum(c);
    let n = c[0].min(c[1]).min(c[2]);
    let x = c[0].max(c[1]).max(c[2]);
    let mut out = c;
    if n < 0.0 {
        let d = l - n;
        for v in &mut out {
            *v = if l > 0.0 && d > 0.0 { l + (*v - l) * l / d } else { l };
        }
    }
    if x > top {
        let d = x - l;
        for v in &mut out {
            *v = if l < top && d > 0.0 { l + (*v - l) * (top - l) / d } else { l };
        }
    }
    out
}

pub fn set_lum(c: [f32; 3], l: f32) -> [f32; 3] {
    set_lum_within(c, l, f32::MAX)
}

fn set_lum_within(c: [f32; 3], l: f32, top: f32) -> [f32; 3] {
    let d = l - lum(c);
    clip_color([c[0] + d, c[1] + d, c[2] + d], top)
}

fn sat(c: [f32; 3]) -> f32 {
    c[0].max(c[1]).max(c[2]) - c[0].min(c[1]).min(c[2])
}

fn set_sat(c: [f32; 3], s: f32) -> [f32; 3] {
    let mut idx = [0usize, 1, 2];
    idx.sort_by(|&a, &b| c[a].partial_cmp(&c[b]).unwrap_or(std::cmp::Ordering::Equal));
    let (imin, imid, imax) = (idx[0], idx[1], idx[2]);
    let mut out = [0.0f32; 3];
    if c[imax] > c[imin] {
        out[imid] = (c[imid] - c[imin]) * s / (c[imax] - c[imin]);
        out[imax] = s;
    }
    out[imin] = 0.0;
    out
}

pub fn blend_rgb(mode: BlendMode, cb: [f32; 3], cs: [f32; 3]) -> [f32; 3] {
    blend_rgb_within(mode, cb, cs, f32::MAX)
}

pub fn blend_rgb_within(mode: BlendMode, cb: [f32; 3], cs: [f32; 3], top: f32) -> [f32; 3] {
    match mode {
        BlendMode::Saturation => set_lum_within(set_sat(cb, sat(cs)), lum(cb), top),
        BlendMode::Color => set_lum_within(cs, lum(cb), top),
        BlendMode::Luminosity => set_lum_within(cb, lum(cs), top),
        m => [
            blend_channel_hdr(m, cb[0], cs[0]),
            blend_channel_hdr(m, cb[1], cs[1]),
            blend_channel_hdr(m, cb[2], cs[2]),
        ],
    }
}

pub fn blend_rgb_hdr(mode: BlendMode, cb: [f32; 3], cs: [f32; 3]) -> [f32; 3] {
    blend_rgb_within(mode, cb, cs, f32::MAX)
}

pub fn composite(mode: BlendMode, backdrop: [f32; 4], source: [f32; 4], opacity: f32) -> [f32; 4] {
    let mix = opacity.clamp(0.0, 1.0);
    if mix <= 0.0 {
        return backdrop;
    }

    let ba = backdrop[3];
    let b = [backdrop[0] * ba, backdrop[1] * ba, backdrop[2] * ba, ba];

    let sa = source[3];
    let a = [source[0] * sa, source[1] * sa, source[2] * sa, sa];

    if sa <= 0.0
        && !matches!(
            mode,
            BlendMode::DarkerColor | BlendMode::PinLight | BlendMode::Darken
        )
    {
        return backdrop;
    }

    let m: [f32; 4] = match mode {
        BlendMode::Normal | BlendMode::PassThrough => [ // over
            a[0] + b[0] * (1.0 - sa),
            a[1] + b[1] * (1.0 - sa),
            a[2] + b[2] * (1.0 - sa),
            sa + ba * (1.0 - sa),
        ],
        BlendMode::Dissolve => [a[0], a[1], a[2], sa], // copy
        BlendMode::Hue => [ // matte
            a[0] + b[0] * (1.0 - sa),
            a[1] + b[1] * (1.0 - sa),
            a[2] + b[2] * (1.0 - sa),
            sa + ba * (1.0 - sa),
        ],
        BlendMode::LinearBurn => [ // under
            a[0] * (1.0 - ba) + b[0],
            a[1] * (1.0 - ba) + b[1],
            a[2] * (1.0 - ba) + b[2],
            sa * (1.0 - ba) + ba,
        ],
        BlendMode::LinearDodge => [ // plus
            a[0] + b[0],
            a[1] + b[1],
            a[2] + b[2],
            (sa + ba).min(1.0),
        ],
        BlendMode::Subtract => [ // minus
            a[0] - b[0],
            a[1] - b[1],
            a[2] - b[2],
            (sa - ba).clamp(0.0, 1.0),
        ],
        BlendMode::PaintNetColorBurn => [ // from
            b[0] - a[0],
            b[1] - a[1],
            b[2] - a[2],
            (ba - sa).clamp(0.0, 1.0),
        ],
        BlendMode::Multiply => [ // multiply
            a[0] * b[0] + b[0] * (1.0 - sa),
            a[1] * b[1] + b[1] * (1.0 - sa),
            a[2] * b[2] + b[2] * (1.0 - sa),
            sa + ba * (1.0 - sa),
        ],
        BlendMode::Screen => [ // screen
            nuke_screen(a[0], b[0]),
            nuke_screen(a[1], b[1]),
            nuke_screen(a[2], b[2]),
            nuke_screen(sa, ba),
        ],
        BlendMode::Darken => [ // min
            a[0].min(b[0]),
            a[1].min(b[1]),
            a[2].min(b[2]),
            sa.min(ba),
        ],
        BlendMode::Lighten => [ // max
            a[0].max(b[0]),
            a[1].max(b[1]),
            a[2].max(b[2]),
            sa.max(ba),
        ],
        BlendMode::Difference => [ // difference
            (a[0] - b[0]).abs(),
            (a[1] - b[1]).abs(),
            (a[2] - b[2]).abs(),
            (sa - ba).abs(),
        ],
        BlendMode::Exclusion => [ // exclusion
            a[0] + b[0] - 2.0 * a[0] * b[0],
            a[1] + b[1] - 2.0 * a[1] * b[1],
            a[2] + b[2] - 2.0 * a[2] * b[2],
            sa + ba - 2.0 * sa * ba,
        ],
        BlendMode::Divide => [ // divide
            nuke_divide(a[0], b[0]),
            nuke_divide(a[1], b[1]),
            nuke_divide(a[2], b[2]),
            sa + ba * (1.0 - sa),
        ],
        BlendMode::LinearLight => [ // hypot
            (a[0] * a[0] + b[0] * b[0]).sqrt(),
            (a[1] * a[1] + b[1] * b[1]).sqrt(),
            (a[2] * a[2] + b[2] * b[2]).sqrt(),
            (sa * sa + ba * ba).sqrt().min(1.0),
        ],
        BlendMode::DarkerColor => [a[0] * ba, a[1] * ba, a[2] * ba, sa * ba], // in
        BlendMode::LighterColor => [ // out
            a[0] * (1.0 - ba),
            a[1] * (1.0 - ba),
            a[2] * (1.0 - ba),
            sa * (1.0 - ba),
        ],
        BlendMode::PinLight => [b[0] * sa, b[1] * sa, b[2] * sa, ba * sa], // mask
        BlendMode::HardMix => [ // stencil
            b[0] * (1.0 - sa),
            b[1] * (1.0 - sa),
            b[2] * (1.0 - sa),
            ba * (1.0 - sa),
        ],
        BlendMode::PaintNetColorDodge => [ // atop
            a[0] * ba + b[0] * (1.0 - sa),
            a[1] * ba + b[1] * (1.0 - sa),
            a[2] * ba + b[2] * (1.0 - sa),
            ba,
        ],
        BlendMode::ColorBurn => [ // color-burn
            if a[0] <= 1e-6 { 0.0 } else { 1.0 - (1.0 - b[0]) / a[0] },
            if a[1] <= 1e-6 { 0.0 } else { 1.0 - (1.0 - b[1]) / a[1] },
            if a[2] <= 1e-6 { 0.0 } else { 1.0 - (1.0 - b[2]) / a[2] },
            sa + ba * (1.0 - sa),
        ],
        BlendMode::ColorDodge => [ // color-dodge
            if 1.0 - a[0] <= 1e-6 { b[0] } else { b[0] / (1.0 - a[0]) },
            if 1.0 - a[1] <= 1e-6 { b[1] } else { b[1] / (1.0 - a[1]) },
            if 1.0 - a[2] <= 1e-6 { b[2] } else { b[2] / (1.0 - a[2]) },
            sa + ba * (1.0 - sa),
        ],
        BlendMode::Xor => [ // xor
            a[0] * (1.0 - ba) + b[0] * (1.0 - sa),
            a[1] * (1.0 - ba) + b[1] * (1.0 - sa),
            a[2] * (1.0 - ba) + b[2] * (1.0 - sa),
            (sa * (1.0 - ba) + ba * (1.0 - sa)).clamp(0.0, 1.0),
        ],
        BlendMode::Glow => { // conjoint-over
            if sa > ba || ba <= 1e-7 {
                a
            } else {
                let k = 1.0 - sa / ba;
                [a[0] + b[0] * k, a[1] + b[1] * k, a[2] + b[2] * k, sa + ba * k]
            }
        }
        BlendMode::Negation => { // disjoint-over
            if sa + ba < 1.0 || ba <= 1e-7 {
                [a[0] + b[0], a[1] + b[1], a[2] + b[2], (sa + ba).min(1.0)]
            } else {
                let k = (1.0 - sa) / ba;
                [a[0] + b[0] * k, a[1] + b[1] * k, a[2] + b[2] * k, 1.0]
            }
        }
        other => {
            let blended = blend_rgb(
                other,
                [backdrop[0], backdrop[1], backdrop[2]],
                [source[0], source[1], source[2]],
            );
            let ao = sa + ba * (1.0 - sa);
            [
                (1.0 - sa) * b[0] + (1.0 - ba) * a[0] + sa * ba * blended[0],
                (1.0 - sa) * b[1] + (1.0 - ba) * a[1] + sa * ba * blended[1],
                (1.0 - sa) * b[2] + (1.0 - ba) * a[2] + sa * ba * blended[2],
                ao,
            ]
        }
    };

    let out_pm = [
        b[0] + (m[0] - b[0]) * mix,
        b[1] + (m[1] - b[1]) * mix,
        b[2] + (m[2] - b[2]) * mix,
        (b[3] + (m[3] - b[3]) * mix).clamp(0.0, 1.0),
    ];

    let ao = out_pm[3];
    if ao <= 1e-7 {
        [0.0; 4]
    } else {
        let inv_a = 1.0 / ao;
        [out_pm[0] * inv_a, out_pm[1] * inv_a, out_pm[2] * inv_a, ao]
    }
}

pub fn composite_atop(mode: BlendMode, backdrop: [f32; 4], source: [f32; 4], opacity: f32) -> [f32; 4] {
    let as_ = (source[3] * opacity).clamp(0.0, 1.0);
    if as_ <= 0.0 {
        return backdrop;
    }
    let b = blend_rgb(
        mode,
        [backdrop[0], backdrop[1], backdrop[2]],
        [source[0], source[1], source[2]],
    );
    let mut out = backdrop;
    for i in 0..3 {
        out[i] = (1.0 - as_) * backdrop[i] + as_ * b[i];
    }
    out
}

#[cfg(test)]
mod nuke_light_tests {
    use super::*;

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-5
    }

    #[test]
    fn hard_light_matches_nuke_merge_math_and_keeps_float() {
        // A = source (top), B = backdrop. Multiply below A = 0.5, screen above, continuous at 0.5.
        let hl = |a: f32, b: f32| blend_channel(BlendMode::HardLight, b, a);
        assert!(close(hl(0.25, 0.4), 0.2));
        // A > 0.5 over a dark backdrop: 1 - 2(1-A)(1-B), not A (the old max() fallback bug).
        assert!(close(hl(0.6, 0.1), 0.28));
        assert!(close(hl(0.9, 0.2), 0.84));
        for b in [0.0f32, 0.1, 0.4, 0.5, 0.9, 1.0, 3.0] {
            assert!(close(hl(0.5, b), b), "neutral at A = 0.5 for B = {b}");
            assert!((hl(0.5 - 1e-4, b) - hl(0.5 + 1e-4, b)).abs() < 1e-3, "continuous at 0.5 for B = {b}");
        }
        // 32-bit: values above 1 are not clamped to 1.
        assert!(hl(0.75, 2.0) > 1.9);
        assert!(hl(3.0, 0.5) > 4.9);
        // Overlay is hard light with A and B swapped (decided by B).
        assert!(close(blend_channel(BlendMode::Overlay, 0.6, 0.1), hl(0.6, 0.1)));
    }

    #[test]
    fn menu_hidden_and_psd_mapping_are_consistent() {
        // Menu and hidden lists are disjoint and together cover every mode but pass-through.
        for m in BlendMode::LAYER_MODES {
            assert!(!BlendMode::HIDDEN_MODES.contains(&m), "{m:?} is both listed and hidden");
        }
        assert_eq!(BlendMode::layer_modes().count(), 33);
        // Every mode with a Photoshop equivalent round-trips through its PSD key.
        for m in BlendMode::layer_modes().filter(|m| m.has_psd_equivalent()) {
            assert_eq!(BlendMode::from_psd_key(m.psd_key()), Some(m), "{}", m.label());
        }
        // Photoshop Subtract (B − A) is Nuke `from`, not `minus`.
        assert_eq!(BlendMode::from_psd_key(*b"fsub"), Some(BlendMode::PaintNetColorBurn));
        assert_eq!(BlendMode::PaintNetColorBurn.label(), "from");
        // Photoshop modes without a Merge equivalent don't import as an unrelated Nuke op.
        for key in [b"diss", b"vLit", b"lLit", b"pLit", b"hMix", b"lbrn", b"fdiv", b"hue "] {
            assert_eq!(BlendMode::from_psd_key(*key), None);
        }
        // Modes without an equivalent are written as Normal.
        assert_eq!(BlendMode::Subtract.psd_key(), *b"norm");
        assert_eq!(BlendMode::Divide.psd_key(), *b"norm");
    }
}
