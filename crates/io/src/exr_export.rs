use exr::prelude::*;
use photocraft_color::blend::BlendMode;
use photocraft_doc::{Document, Layer as DocLayer, LayerContent};
use std::path::Path;

const BEAUTY: &str = "rgba";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NukeCompression {
    None,
    Rle,
    ZipScanline,
    ZipBlock,
    Piz,
}

impl NukeCompression {
    fn to_exr_compression(self) -> Compression {
        match self {
            NukeCompression::None => Compression::Uncompressed,
            NukeCompression::Rle => Compression::RLE,
            NukeCompression::ZipScanline => Compression::ZIP1,
            NukeCompression::ZipBlock => Compression::ZIP16,
            NukeCompression::Piz => Compression::PIZ,
        }
    }
}

/// `name` must already be sanitized (see `sanitize_layer_name`); it is used
/// verbatim as the EXR layer prefix, the Shuffle `in`, and in node names.
pub struct ExportLayer {
    pub name: String,
    pub blend_mode: BlendMode,
    /// Layer opacity × fill opacity: the Merge `mix`.
    pub opacity: f32,
    /// Hidden layers are still written to the EXR, but their Merge is disabled and the flat
    /// beauty leaves them out, as the Layers panel does.
    pub visible: bool,
    /// A Grade or RGB Curves adjustment layer: no pixels in the EXR, a Nuke `Grade` or
    /// `ColorLookup` node on the comp chain instead of a Shuffle → Premult → Merge column
    /// (`opacity` is its mix).
    pub adjustment: Option<photocraft_doc::Adjustment>,
}

/// A Grade knob as `.nk` text: one value when the channels agree, else `{r g b a}` with
/// alpha at the knob's neutral value (the node grades rgb only).
fn nk_knob(v: &[f32; 3], neutral: f32) -> String {
    if v[0] == v[1] && v[1] == v[2] { format!("{}", v[0]) } else { format!("{{{} {} {} {neutral}}}", v[0], v[1], v[2]) }
}

/// The body of a Nuke `Grade` node for a Grade adjustment (knobs at their defaults omitted).
fn nk_grade_knobs(adj: &photocraft_doc::Adjustment) -> String {
    let photocraft_doc::Adjustment::Grade { blackpoint, whitepoint, lift, gain, multiply, offset, gamma, black_clamp, white_clamp } = adj else {
        return String::new();
    };
    let mut s = String::from(" channels rgb\n");
    for (key, v, neutral) in [
        ("blackpoint", blackpoint, 0.0),
        ("whitepoint", whitepoint, 1.0),
        ("black", lift, 0.0),
        ("white", gain, 1.0),
        ("multiply", multiply, 1.0),
        ("add", offset, 0.0),
        ("gamma", gamma, 1.0),
    ] {
        if v.iter().any(|c| *c != neutral) {
            s.push_str(&format!(" {key} {}\n", nk_knob(v, neutral)));
        }
    }
    if !black_clamp {
        s.push_str(" black_clamp false\n");
    }
    if *white_clamp {
        s.push_str(" white_clamp true\n");
    }
    // PhotoCraft grades the composite's straight colour; the comp pipe is premultiplied.
    s.push_str(" unpremult rgba.alpha\n");
    s
}

/// Only ASCII alphanumerics and '_' survive. No dots (they create sub-layers),
/// no spaces, no leading digit, never the reserved beauty name.
fn sanitize_layer_name(raw: &str) -> String {
    let mut s: String = raw
        .trim()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '_' { c } else { '_' })
        .collect();
    if s.is_empty() {
        s = "Layer".into();
    }
    if s.chars().next().map_or(false, |c| c.is_ascii_digit()) {
        s = format!("L_{}", s);
    }
    if s.eq_ignore_ascii_case(BEAUTY) {
        s = format!("{}_layer", s);
    }
    s
}

/// Returns (Nuke Merge2 operation, is_exact_match).
///
/// Photocraft's layer modes are Nuke Merge operations, and each mode's label is the Merge2
/// `operation` name, so the script uses the same maths the canvas renders. Only pass-through
/// (groups are flattened into the layer list, so it is plain over) and Saturation / Color /
/// Luminosity (no Merge equivalent) fall back to over, flagged with `~` in the breakdown.
pub fn nuke_merge_operation(mode: BlendMode) -> (&'static str, bool) {
    match mode {
        BlendMode::PassThrough => ("over", true),
        BlendMode::Saturation | BlendMode::Color | BlendMode::Luminosity => ("over", false),
        m => (m.label(), true),
    }
}

fn tcl_escape(s: &str) -> String {
    s.replace('\\', "/")
        .replace('"', "\\\"")
        .replace('[', "\\[")
        .replace('$', "\\$")
}

/// Backdrop colour by blend family, so the breakdown reads at a glance.
fn backdrop_color(op: &str, is_base: bool) -> &'static str {
    if is_base {
        return "0x2e2e2eff";
    }
    match op {
        "multiply" | "min" | "color-burn" => "0x34465aff",            // darken: blue
        "screen" | "plus" | "max" | "color-dodge" => "0x5a4a32ff",    // lighten: amber
        "overlay" | "soft-light" | "hard-light" => "0x4c3a5aff",      // contrast: purple
        "difference" | "exclusion" | "from" | "minus" | "divide" | "average" | "geometric" | "hypot" => {
            "0x335a4aff" // math: green
        }
        "grade" | "colorlookup" => "0x5a3a3aff",                       // colour correction: red
        _ => "0x404040ff",                                            // over: grey
    }
}

/// Generates a Nuke script laid out like a PSD breakdown:
///
///   Read
///    o-------o-------o-------o        bus (one Dot per layer)
///   [Layer] [Layer] [Layer] [Layer]   one backdrop column each:
///   Shuffle Shuffle Shuffle Shuffle     Shuffle -> Premult
///   Premult Premult Premult Premult
///  [ o----->Merge-->Merge-->Merge ]   COMP row, B runs left to right
pub fn generate_nuke_script(
    exr_filename: &str,
    layers: &[ExportLayer],
    width: u32,
    height: u32,
) -> String {
    let (w, h) = (width, height);
    let fmt = format!("{w} {h} 0 0 {w} {h} 1 custom");
    let mut nk = String::new();

    nk.push_str(&format!(
        "Root {{\n inputs 0\n format \"{fmt}\"\n project_directory \"[python {{nuke.script_directory()}}]\"\n colorManagement Nuke\n workingSpaceLUT linear\n}}\n"
    ));

    // Register every layer before any node references it, otherwise a Shuffle's
    // `in <layer>` resolves to `none` for names Nuke hasn't seen this session.
    for layer in layers.iter().filter(|l| l.adjustment.is_none()) {
        let n = &layer.name;
        nk.push_str(&format!(
            "add_layer {{{n} {n}.red {n}.green {n}.blue {n}.alpha}}\n"
        ));
    }

    // Layout grid (Nuke nodes are 80x18, Dots 12x12).
    const COL_W: i32 = 200; // column pitch: room for long node names + Merge autolabel
    const DOT_DX: i32 = 34; // centres a Dot over an 80-wide node
    const READ_Y: i32 = -230;
    const BUS_Y: i32 = -100;
    const BD_Y: i32 = -50; // layer backdrop top
    const BD_H: i32 = 190;
    const SHUFFLE_Y: i32 = 40;
    const PREMULT_Y: i32 = 100;
    const COMP_BD_Y: i32 = 170;
    const MERGE_Y: i32 = 220;

    let n = layers.len() as i32;

    nk.push_str(&format!(
        "Read {{\n inputs 0\n file_type exr\n file \"[python {{nuke.script_directory()}}]/{}\"\n format \"{fmt}\"\n origset true\n colorspace linear\n name Read_MultiChannel\n xpos 0\n ypos {READ_Y}\n}}\nset NRead_MultiChannel [stack 0]\n",
        tcl_escape(exr_filename)
    ));

    let mut backdrops = String::new();
    let mut prev_bus = String::from("NRead_MultiChannel");
    let mut comp = String::new();

    for (i, layer) in layers.iter().enumerate() {
        let name = &layer.name;
        let x = (i as i32) * COL_W;

        // An adjustment: a Grade or ColorLookup node on the comp chain (nothing to adjust before
        // the base).
        if let Some(adj) = &layer.adjustment {
            if comp.is_empty() {
                continue;
            }
            let (class, knobs, op) = match adj {
                photocraft_doc::Adjustment::Grade { .. } => ("Grade", nk_grade_knobs(adj), "grade"),
                _ => ("ColorLookup", nk_lookup_knobs(adj), "colorlookup"),
            };
            nk.push_str(&format!(
                "push ${comp}\n{class} {{\n{knobs}{} mix {}\n name {class}_{name}\n xpos {x}\n ypos {MERGE_Y}\n}}\nset N{class}_{name} [stack 0]\n",
                if layer.visible { "" } else { " disable true\n" },
                layer.opacity.clamp(0.0, 1.0)
            ));
            comp = format!("N{class}_{name}");
            let pct = (layer.opacity.clamp(0.0, 1.0) * 100.0).round() as i32;
            let sub = if !layer.visible {
                "hidden".to_string()
            } else if pct < 100 {
                format!("{op}  {pct}%")
            } else {
                op.to_string()
            };
            backdrops.push_str(&format!(
                "BackdropNode {{\n inputs 0\n name BD_{name}\n tile_color {}\n label \"<center><b>{name}</b>\\n<font size=2>{sub}</font></center>\"\n note_font_size 18\n xpos {}\n ypos {BD_Y}\n bdwidth {}\n bdheight {BD_H}\n z_order 1\n}}\n",
                backdrop_color(op, false),
                x - 50,
                COL_W - 20
            ));
            continue;
        }

        let is_base = comp.is_empty();
        let (op, exact) = nuke_merge_operation(layer.blend_mode);

        // Bus Dot, chained off the previous one (first off the Read).
        nk.push_str(&format!(
            "push ${prev_bus}\nDot {{\n name Dot_{name}\n xpos {}\n ypos {BUS_Y}\n}}\nset NDot_{name} [stack 0]\n",
            x + DOT_DX
        ));
        prev_bus = format!("NDot_{name}");

        // Shuffle -> Premult (EXR holds straight alpha; Merge wants premultiplied).
        nk.push_str(&format!(
            "Shuffle {{\n in {name}\n out rgba\n name Shuffle_{name}\n xpos {x}\n ypos {SHUFFLE_Y}\n}}\n"
        ));
        nk.push_str(&format!(
            "Premult {{\n name Premult_{name}\n xpos {x}\n ypos {PREMULT_Y}\n}}\nset NPremult_{name} [stack 0]\n"
        ));

        if is_base {
            // Base drops straight into the COMP row as the start of the B pipe. Over an empty
            // backdrop every mode is plain over, so only its opacity (0 when hidden) applies.
            let base_mix = if layer.visible { layer.opacity.clamp(0.0, 1.0) } else { 0.0 };
            if base_mix < 1.0 {
                nk.push_str(&format!(
                    "Multiply {{\n channels rgba\n value {base_mix}\n name Opacity_{name}\n xpos {x}\n ypos {}\n}}\n",
                    PREMULT_Y + 30
                ));
            }
            nk.push_str(&format!(
                "Dot {{\n name Dot_Comp_{name}\n xpos {}\n ypos {}\n}}\nset NDot_Comp_{name} [stack 0]\n",
                x + DOT_DX,
                MERGE_Y + 3
            ));
            comp = format!("NDot_Comp_{name}");
        } else {
            // Merge2 input 0 = B (comp so far), input 1 = A (this layer).
            // Input 0 is taken from the top of the stack: push A, then B.
            nk.push_str(&format!(
                "push $NPremult_{name}\npush ${comp}\nMerge2 {{\n inputs 2\n operation {op}\n mix {}\n{} name Merge_{name}\n xpos {x}\n ypos {MERGE_Y}\n}}\nset NMerge_{name} [stack 0]\n",
                layer.opacity.clamp(0.0, 1.0),
                if layer.visible { "" } else { " disable true\n" }
            ));
            comp = format!("NMerge_{name}");
        }

        let sub = if !layer.visible {
            "hidden".to_string()
        } else if is_base {
            "base".to_string()
        } else {
            let pct = (layer.opacity.clamp(0.0, 1.0) * 100.0).round() as i32;
            let approx = if exact { "" } else { " ~" };
            if pct < 100 { format!("{op}{approx}  {pct}%") } else { format!("{op}{approx}") }
        };
        backdrops.push_str(&format!(
            "BackdropNode {{\n inputs 0\n name BD_{name}\n tile_color {}\n label \"<center><b>{name}</b>\\n<font size=2>{sub}</font></center>\"\n note_font_size 18\n xpos {}\n ypos {BD_Y}\n bdwidth {}\n bdheight {BD_H}\n z_order 1\n}}\n",
            backdrop_color(op, is_base),
            x - 50,
            COL_W - 20
        ));
    }

    // Output Dot at the end of the comp chain, ready for a Write/Viewer.
    nk.push_str(&format!(
        "Dot {{\n name Dot_OUT\n label OUT\n note_font_size 18\n xpos {}\n ypos {}\n}}\n",
        n * COL_W - COL_W / 2 + DOT_DX,
        MERGE_Y + 3
    ));

    // One long COMP backdrop under all columns, behind the layer backdrops.
    nk.push_str(&format!(
        "BackdropNode {{\n inputs 0\n name BD_COMP\n tile_color 0x262626ff\n label \"<b>COMP</b>\"\n note_font_size 18\n xpos -50\n ypos {COMP_BD_Y}\n bdwidth {}\n bdheight 90\n z_order 0\n}}\n",
        n * COL_W
    ));
    nk.push_str(&backdrops);
    nk
}

/// `px` (straight linear RGBA) through `adj` as a 32-bit linear document applies it.
fn adjusted_px(adj: &photocraft_doc::Adjustment, px: &[[f32; 4]]) -> Vec<[f32; 4]> {
    use photocraft_color::{ColorMode, SampleType};
    use photocraft_compose::adjust::{Transfer, apply_depth};
    let mut buf = photocraft_compose::Buffer { rect: photocraft_geom::Rect::from_xywh(0, 0, px.len().max(1) as u32, 1), px: px.to_vec() };
    apply_depth(adj, &mut buf, Transfer::for_document(ColorMode::Rgb, SampleType::F32), Some(SampleType::F32));
    buf.px
}

/// The body of a Nuke `ColorLookup` node reproducing a Curves adjustment as PhotoCraft applies
/// it in 32-bit: each channel's response (master and channel curves composed in the layer's
/// order) sampled at 129 points over the curves' range with linear keys, plus a key far below
/// and far above so its ends (held or continued along their slopes) carry over whatever
/// extrapolation Nuke uses. The master and alpha curves are identity.
fn nk_lookup_knobs(adj: &photocraft_doc::Adjustment) -> String {
    let range = match adj {
        photocraft_doc::Adjustment::Curves { lookup, pencil: false, .. } => lookup.span(),
        _ => 1.0,
    };
    let mut xs = vec![-100.0 * range];
    xs.extend((0..=128).map(|k| k as f32 / 128.0 * range));
    xs.push(100.0 * range);
    let px: Vec<[f32; 4]> = xs.iter().map(|&x| [x, x, x, 1.0]).collect();
    let ys = adjusted_px(adj, &px);
    let curve = |c: usize| {
        let keys: Vec<String> = xs.iter().zip(&ys).map(|(x, y)| format!("x{x} {}", y[c])).collect();
        format!("{{curve L {}}}", keys.join(" "))
    };
    let identity = "{curve L x-100000 -100000 x100000 100000}";
    format!(" channels rgb\n lut {{master {identity}\n  red {}\n  green {}\n  blue {}\n  alpha {identity}}}\n unpremult rgba.alpha\n", curve(0), curve(1), curve(2))
}

/// `base`, or `base_2`, `base_3`, … when an earlier layer already has that name (EXR layer
/// prefixes and node names must be unique).
fn unique_name(layers: &[ExportLayer], base: &str) -> String {
    let mut name = base.to_string();
    let mut suffix = 2usize;
    while layers.iter().any(|l| l.name == name) {
        name = format!("{base}_{suffix}");
        suffix += 1;
    }
    name
}

pub fn export_multichannel_exr_with_options(
    doc: &Document,
    path: &Path,
    compression: NukeCompression,
    include_flat_beauty: bool,
    use_32bit: bool,
    write_nuke_script: bool,
) -> std::result::Result<(), String> {
    let width = doc.size.width as usize;
    let height = doc.size.height as usize;

    if width == 0 || height == 0 {
        return Err("Document dimensions must be greater than zero.".into());
    }

    let bounds = doc.bounds();
    let mut extracted: Vec<(String, Vec<[f32; 4]>)> = Vec::new();
    let mut nuke_layers: Vec<ExportLayer> = Vec::new();

    // (layer, every enclosing group visible)
    let mut stack: Vec<(&DocLayer, bool)> = doc.layers.iter().rev().map(|l| (l, true)).collect();
    while let Some((doc_layer, parent_visible)) = stack.pop() {
        if let LayerContent::Raster(surface) = &doc_layer.content {
            let px_data = photocraft_compose::surface_to_buffer(surface, bounds).px;

            let raw = if doc_layer.name.trim().is_empty() {
                format!("Layer_{}", extracted.len() + 1)
            } else {
                doc_layer.name.clone()
            };
            let base_name = sanitize_layer_name(&raw);

            let layer_name = unique_name(&nuke_layers, &base_name);
            nuke_layers.push(ExportLayer {
                name: layer_name.clone(),
                blend_mode: doc_layer.blend,
                opacity: doc_layer.opacity * doc_layer.fill_opacity,
                visible: doc_layer.visible && parent_visible,
                adjustment: None,
            });
            extracted.push((layer_name, px_data));
        } else if let LayerContent::Adjustment(
            adj @ (photocraft_doc::Adjustment::Grade { .. }
            | photocraft_doc::Adjustment::Curves { space: photocraft_doc::adjust::ToneSpace::Rgb, .. }),
        ) = &doc_layer.content
        {
            let raw = if doc_layer.name.trim().is_empty() { adj.label().to_string() } else { doc_layer.name.clone() };
            nuke_layers.push(ExportLayer {
                name: unique_name(&nuke_layers, &sanitize_layer_name(&raw)),
                blend_mode: BlendMode::Normal,
                opacity: doc_layer.opacity * doc_layer.fill_opacity,
                visible: doc_layer.visible && parent_visible,
                adjustment: Some(adj.clone()),
            });
        }

        if let Some(children) = doc_layer.children() {
            let visible = parent_visible && doc_layer.visible;
            stack.extend(children.iter().rev().map(|c| (c, visible)));
        }
    }

    if extracted.is_empty() {
        return Err("No layers available for export.".into());
    }

    if include_flat_beauty {
        // The same chain the .nk script builds: the base layer over empty, then one Merge per
        // layer with its operation and mix, using the Merge maths the canvas renders
        // (photocraft_color::blend::composite, straight alpha in and out). Hidden layers are
        // skipped, as their Merges are disabled.
        // Grade and Curves adjustments change the composite's straight colour by their mix,
        // through the compositor's own adjustment maths, as their nodes do (unpremult by
        // alpha); one before the base layer has nothing to adjust.
        let mut flat_data = vec![[0.0_f32; 4]; width * height];
        let mut pixels = extracted.iter();
        let mut first = true;
        for layer in &nuke_layers {
            if let Some(adj) = &layer.adjustment {
                if first || !layer.visible {
                    continue;
                }
                let mix = layer.opacity.clamp(0.0, 1.0);
                let adjusted = adjusted_px(adj, &flat_data);
                for (p, a) in flat_data.iter_mut().zip(adjusted).filter(|(p, _)| p[3] > 0.0) {
                    for c in 0..3 {
                        p[c] += (a[c] - p[c]) * mix;
                    }
                }
                continue;
            }
            let Some((_, layer_data)) = pixels.next() else { break };
            let mode = if first { BlendMode::Normal } else { layer.blend_mode };
            first = false;
            if !layer.visible {
                continue;
            }
            let mix = layer.opacity.clamp(0.0, 1.0);
            for (dst, src) in flat_data.iter_mut().zip(layer_data) {
                *dst = photocraft_color::blend::composite(mode, *dst, *src, mix);
            }
        }
        extracted.push((BEAUTY.to_string(), flat_data));
    }

    let to_samples = |vals: Vec<f32>| -> FlatSamples {
        if use_32bit {
            FlatSamples::F32(vals)
        } else {
            FlatSamples::F16(vals.into_iter().map(f16::from_f32).collect())
        }
    };

    // Single-part EXR, fully qualified channel names: "<Layer>.R/G/B/A",
    // beauty as bare R/G/B/A. This is what Nuke's Shuffle `in <Layer>` expects.
    let mut chans: SmallVec<[AnyChannel<FlatSamples>; 4]> = SmallVec::new();
    for (name, data) in &extracted {
        let prefix = if name == BEAUTY { String::new() } else { format!("{name}.") };
        for (idx, ch) in ["R", "G", "B", "A"].iter().enumerate() {
            let label = format!("{prefix}{ch}");
            let vals: Vec<f32> = data.iter().map(|p| p[idx]).collect();
            chans.push(AnyChannel::new(label.as_str(), to_samples(vals)));
        }
    }

    let layer = Layer::new(
        Vec2(width, height),
        LayerAttributes::default(),
        Encoding {
            compression: compression.to_exr_compression(),
            line_order: LineOrder::Increasing,
            blocks: Blocks::ScanLines,
        },
        AnyChannels::sort(chans),
    );
    let image = Image::from_layer(layer);

    image.write().to_file(path).map_err(|e| e.to_string())?;

    if write_nuke_script {
        let exr_filename = path.file_name().unwrap_or_default().to_string_lossy();
        let nk_path = path.with_extension("nk");
        let nk_content =
            generate_nuke_script(&exr_filename, &nuke_layers, width as u32, height as u32);
        std::fs::write(&nk_path, nk_content).map_err(|e| e.to_string())?;
    }

    Ok(())
}

pub fn export_multichannel_exr(doc: &Document, path: &Path) -> std::result::Result<(), String> {
    export_multichannel_exr_with_options(doc, path, NukeCompression::ZipScanline, true, true, false)
}