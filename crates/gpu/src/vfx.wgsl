// PhotoCraft VFX's additions to the compositor shader, kept out of compose.wgsl so upstream
// PhotoCraft's file changes as little as possible and merges stay easy. kit::SHADER compiles this
// file after compose.wgsl as one module (WGSL declarations may come in any order), so everything
// here can use compose.wgsl's bindings and helpers, and compose.wgsl calls in through a few
// one-line hooks: `mode >= NUKE_BASE` in blend_rgb, composite and composite_fill, the 32-bit
// helpers in blend_channel, and cases 18 and 19 of adjust. See FORK.md at the repository root.

// ---- 32-bit (HDR) Photoshop modes ----

// photocraft_color::blend::color_burn / color_dodge: the exact edges 32-bit documents use (the
// integer depths take psblend's EDGE versions above, as the CPU does).
fn color_burn_exact(cb: f32, cs: f32) -> f32 {
    if (cb >= 1.0) { return 1.0; }
    if (cs <= 0.0) { return 0.0; }
    return 1.0 - min((1.0 - cb) / cs, 1.0);
}
fn color_dodge_exact(cb: f32, cs: f32) -> f32 {
    if (cb <= 0.0) { return 0.0; }
    if (cs >= 1.0) { return 1.0; }
    return min(cb / (1.0 - cs), 1.0);
}
// photocraft_color::blend's Vivid Light (32-bit documents): burn below a half, dodge above.
fn vivid_light_exact(cb: f32, cs: f32) -> f32 {
    if (cs <= 0.0) { return 0.0; }
    if (cs >= 1.0) { return 1.0; }
    if (cs <= 0.5) { return color_burn_exact(cb, 2.0 * cs); }
    return color_dodge_exact(cb, 2.0 * (cs - 0.5));
}
// photocraft_color::blend::range_bound: the Photoshop modes whose formula means something on 0..1
// only (Screen, Color Dodge, Overlay, Soft Light, Hard Light, Vivid Light, Linear Light,
// Exclusion, Reflect, Glow, Negation). In 32-bit documents, outside 0..1 the brighter input stands.
fn range_bound(mode: i32) -> bool {
    return mode == 9 || mode == 10 || (mode >= 13 && mode <= 17) || mode == 21 || (mode >= 28 && mode <= 30);
}

// The Nuke Merge operations start here (gpu::plan::lower::mode_index; photocraft_color::blend::nuke_channel).
const NUKE_BASE: i32 = 34;

// ---- Nuke Merge operations (A = source, B = backdrop; Nuke's 32-bit maths, measured in Nuke 17) ----
// Mirrors photocraft_color::blend; see its comments for what each formula is. Written on whole
// colours with `select`, never per channel or in loops: HLSL inlines every call, and DX12's FXC
// gives up ("compilation aborted unexpectedly") on fs_blend once these are copied per channel or
// indexed dynamically.

fn nuke_screen(a: f32, b: f32) -> f32 {
    if (a >= 0.0 && a <= 1.0 && b >= 0.0 && b <= 1.0) { return a + b - a * b; }
    return max(a, b);
}

fn in01(v: vec3<f32>) -> vec3<bool> { return (v >= vec3(0.0)) & (v <= vec3(1.0)); }
fn neg(v: vec3<f32>) -> vec3<bool> { return v < vec3(0.0); }

fn nk_screen(a: vec3<f32>, b: vec3<f32>) -> vec3<f32> { return select(max(a, b), a + b - a * b, in01(a) & in01(b)); }
fn nk_multiply(a: vec3<f32>, b: vec3<f32>) -> vec3<f32> { return select(a * b, a, neg(a) & neg(b)); }

// overlay / hard-light core: deciding input x clamped to 0..1, y to 0 and above, result to 1 and below.
fn nk_light(x_in: vec3<f32>, y_in: vec3<f32>) -> vec3<f32> {
    let x = clamp(x_in, vec3(0.0), vec3(1.0));
    let y = max(y_in, vec3(0.0));
    return min(select(1.0 - 2.0 * (1.0 - x) * (1.0 - y), 2.0 * x * y, x <= vec3(0.5)), vec3(1.0));
}

fn nk_soft_light_p(a: vec3<f32>, b: vec3<f32>, p: vec3<f32>) -> vec3<f32> {
    return select(2.0 * a * b, b * (2.0 * a + b * (1.0 - p)), p < vec3(1.0));
}

fn nk_divide(a: vec3<f32>, b: vec3<f32>) -> vec3<f32> {
    let pos = b > vec3(0.0);
    return select(a, a / select(vec3(1.0), b, pos), pos);
}

fn nk_geometric(a: vec3<f32>, b: vec3<f32>) -> vec3<f32> {
    let sum = a + b;
    let zero = neg(a) | neg(b) | (sum == vec3(0.0));
    return select(2.0 * a * b / select(sum, vec3(1.0), zero), vec3(0.0), zero);
}

fn nk_color_dodge(a_in: vec3<f32>, b_in: vec3<f32>) -> vec3<f32> {
    let a = max(a_in, vec3(0.0));
    let b = max(b_in, vec3(0.0));
    let one = a >= vec3(1.0);
    return select(min(b / select(1.0 - a, vec3(1.0), one), vec3(1.0)), vec3(1.0), one);
}

fn nk_color_burn(a: vec3<f32>, b: vec3<f32>) -> vec3<f32> {
    let zero = a <= vec3(0.0);
    return select(max(1.0 - (1.0 - max(b, vec3(0.0))) / select(a, vec3(1.0), zero), vec3(0.0)), vec3(0.0), zero);
}

// overlay, hard-light, soft-light, color-dodge, color-burn: Nuke clamps both colours to 0 and above.
fn nuke_clamps_inputs(mode: i32) -> bool {
    return mode == 44 || mode == 46 || mode == 45 || mode == 41 || mode == 36;
}

// photocraft_color::blend::nuke_channel on a whole colour (A = cs, B = cb).
fn nuke_rgb(mode: i32, cb: vec3<f32>, cs: vec3<f32>) -> vec3<f32> {
    // Each case assigns and the one return is at the end (see blend_channel).
    var r = cs;
    switch mode {
        case 34, 38: { r = min(cb, cs); }                                        // min / in
        case 35: { r = nk_multiply(cs, cb); }                                     // multiply
        case 60: { r = cb - cs; }                                                 // from
        case 37, 61: { r = cb; }                                                  // under / atop
        case 39, 43: { r = max(cb, cs); }                                         // max / out
        case 40: { r = nk_screen(cs, cb); }                                       // screen
        case 41: { r = nk_color_dodge(cs, cb); }                                  // color-dodge
        case 36: { r = nk_color_burn(cs, cb); }                                   // color-burn
        case 42: { r = cs + cb; }                                                 // plus
        case 44: { r = nk_light(cb, cs); }                                        // overlay (decided by B)
        case 45: {                                                                // soft-light
            let a = max(cs, vec3(0.0));
            let b = max(cb, vec3(0.0));
            r = nk_soft_light_p(a, b, a * b);
        }
        case 46: { r = nk_light(cs, cb); }                                        // hard-light (decided by A)
        case 47: { r = nk_geometric(cs, cb); }                                    // geometric
        case 48: { r = sqrt(cs * cs + cb * cb); }                                 // hypot
        case 51: { r = abs(cs - cb); }                                            // difference
        case 52: { r = select(cs + cb - 2.0 * cs * cb, cs + cb, neg(cs) | neg(cb)); } // exclusion
        case 53: { r = cs - cb; }                                                 // minus
        case 54: { r = nk_divide(cs, cb); }                                       // divide
        case 56: { r = 0.5 * (cs + cb); }                                         // average
        default: { r = cs; }
    }
    return r;
}

// The alpha-masked share where both inputs cover (blend.rs nuke_composite): `cs`/`cb` straight,
// `ap`/`bp` premultiplied, `both` = ab.
fn nuke_masked_share(mode: i32, cb: vec3<f32>, cs: vec3<f32>, bp: vec3<f32>, ap: vec3<f32>, both: f32) -> vec3<f32> {
    var r = both * nuke_rgb(mode, cb, cs);
    if (mode == 53) {                                                         // minus
        r = both * (ap - bp);
    } else if (mode == 60) {                                                  // from
        r = both * (bp - ap);
    } else if (mode == 54) {                                                  // divide
        r = select(both * cs, ap / select(vec3(1.0), bp, bp > vec3(0.0)), bp > vec3(0.0));
    } else if (mode == 47) {                                                  // geometric
        r = nk_geometric(ap, bp);
    } else if (mode == 45) {                                                  // soft-light
        r = both * nk_soft_light_p(cs, cb, cs * bp);
    }
    return r;
}

fn nuke_composite(mode: i32, b_in: vec4<f32>, s_in: vec4<f32>, opacity: f32) -> vec4<f32> {
    let mix = clamp(opacity, 0.0, 1.0);
    if (mix <= 0.0) { return b_in; }

    let ba = b_in.a;
    let b = vec4(b_in.rgb * ba, ba);
    let sa = s_in.a;
    let a = vec4(s_in.rgb * sa, sa);

    // in, mask, out, min, max and hypot change the backdrop even where the source is empty (blend.rs).
    if (sa <= 0.0 && mode != 38 && mode != 49 && mode != 43 && mode != 34 && mode != 39 && mode != 48) {
        return b_in;
    }

    var m = vec4(0.0);
    switch mode {
        case 55: { // matte
            m = vec4(a.rgb + b.rgb * (1.0 - sa), sa + ba * (1.0 - sa));
        }
        case 37: { // under
            m = vec4(a.rgb * (1.0 - ba) + b.rgb, sa * (1.0 - ba) + ba);
        }
        case 42: { // plus
            m = vec4(a.rgb + b.rgb, min(sa + ba, 1.0));
        }
        case 40: { // screen
            m = vec4(nuke_screen(a.r, b.r), nuke_screen(a.g, b.g), nuke_screen(a.b, b.b), nuke_screen(sa, ba));
        }
        case 34: { // min
            m = vec4(min(a.rgb, b.rgb), min(sa, ba));
        }
        case 39: { // max
            m = vec4(max(a.rgb, b.rgb), max(sa, ba));
        }
        case 48: { // hypot
            m = vec4(sqrt(a.rgb * a.rgb + b.rgb * b.rgb), min(sqrt(sa * sa + ba * ba), 1.0));
        }
        case 38: { // in
            m = vec4(a.rgb * ba, sa * ba);
        }
        case 43: { // out
            m = vec4(a.rgb * (1.0 - ba), sa * (1.0 - ba));
        }
        case 49: { // mask
            m = vec4(b.rgb * sa, ba * sa);
        }
        case 50: { // stencil
            m = vec4(b.rgb * (1.0 - sa), ba * (1.0 - sa));
        }
        case 61: { // atop
            m = vec4(a.rgb * ba + b.rgb * (1.0 - sa), ba);
        }
        case 59: { // xor
            m = vec4(a.rgb * (1.0 - ba) + b.rgb * (1.0 - sa), clamp(sa * (1.0 - ba) + ba * (1.0 - sa), 0.0, 1.0));
        }
        case 57: { // conjoint-over
            if (sa > ba || ba <= 1e-7) {
                m = vec4(a.rgb, sa);
            } else {
                let k = 1.0 - sa / ba;
                m = vec4(a.rgb + b.rgb * k, sa + ba * k);
            }
        }
        case 58: { // disjoint-over
            if (sa + ba < 1.0 || ba <= 1e-7) {
                m = vec4(a.rgb + b.rgb, min(sa + ba, 1.0));
            } else {
                let k = (1.0 - sa) / ba;
                m = vec4(a.rgb + b.rgb * k, 1.0);
            }
        }
        default: { // the alpha-masked ops (see blend.rs nuke_composite)
            var cs = s_in.rgb;
            var cb = b_in.rgb;
            if (nuke_clamps_inputs(mode)) {
                cs = max(cs, vec3(0.0));
                cb = max(cb, vec3(0.0));
            }
            let ap = cs * sa;
            let bp = cb * ba;
            let both = sa * ba;
            let share = nuke_masked_share(mode, cb, cs, bp, ap, both);
            let ao = sa + ba * (1.0 - sa);
            m = vec4((1.0 - sa) * bp + (1.0 - ba) * ap + share, ao);
        }
    }

    let out_pm = vec4(b.rgb + (m.rgb - b.rgb) * mix, clamp(b.a + (m.a - b.a) * mix, 0.0, 1.0));

    if (out_pm.a <= 1e-7) { return vec4(0.0); }
    return vec4(out_pm.rgb / out_pm.a, out_pm.a);
}

// ---- Nuke Grade and ColorLookup adjustments ----

// Nuke Grade (compose::adjust::GradeCoefficients): p0 = a, p1 = b, p2 = 1/gamma per channel,
// p3 = (black clamp, white clamp, tone-curve exponent, float document). Integer documents are
// decoded to linear light first and re-encoded after, as on the CPU.
fn grade(c: vec3<f32>, p0: vec4<f32>, p1: vec4<f32>, p2: vec4<f32>, p3: vec4<f32>) -> vec3<f32> {
    let float_doc = p3.w > 0.5;
    var x = c;
    if (!float_doc) { x = vec3(t_decode(c.r, p3.z), t_decode(c.g, p3.z), t_decode(c.b, p3.z)); }
    var v = p0.xyz * x + p1.xyz;
    // Gamma on positive values only (negatives pass through, as in Nuke).
    if (p2.x != 1.0 && v.x > 0.0) { v.x = pow(v.x, p2.x); }
    if (p2.y != 1.0 && v.y > 0.0) { v.y = pow(v.y, p2.y); }
    if (p2.z != 1.0 && v.z > 0.0) { v.z = pow(v.z, p2.z); }
    if (p3.x > 0.5) { v = max(v, vec3(0.0)); }
    if (p3.y > 0.5) { v = min(v, vec3(1.0)); }
    if (!float_doc) {
        v = max(v, vec3(0.0));
        v = clamp(vec3(t_encode(v.r, p3.z), t_encode(v.g, p3.z), t_encode(v.b, p3.z)), vec3(0.0), vec3(1.0));
    }
    return v;
}

// compose::adjust::lookup_sample: a row tabulated over 0..range, held or continued along the
// end slopes (over 16 table steps) outside it.
fn lookup_channel(row: i32, v: f32, range: f32, hold: bool) -> f32 {
    let t = v / range;
    if (!hold && abs(v) <= 3.0e38) {
        let per_unit = 4095.0 / (16.0 * range);
        let first = textureLoad(lut_tex, vec2(0, row), 0).r;
        let last = textureLoad(lut_tex, vec2(4095, row), 0).r;
        if (t < 0.0) { return first + v * (textureLoad(lut_tex, vec2(16, row), 0).r - first) * per_unit; }
        if (t > 1.0) { return last + (v - range) * (last - textureLoad(lut_tex, vec2(4079, row), 0).r) * per_unit; }
    }
    return lut(row, t);
}

// Nuke ColorLookup-style Curves (adjust case 19): p0 = (range, hold ends, float document).
fn lookup_curves(c: vec3<f32>, p0: vec4<f32>) -> vec3<f32> {
    let hold = p0.y > 0.5;
    let o = vec3(lookup_channel(0, c.r, p0.x, hold), lookup_channel(1, c.g, p0.x, hold), lookup_channel(2, c.b, p0.x, hold));
    if (p0.z > 0.5) { return o; }
    return clamp(o, vec3(0.0), vec3(1.0));
}
