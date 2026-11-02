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
    for layer in layers {
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
        let is_base = i == 0;
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

            let mut layer_name = base_name.clone();
            let mut suffix = 2usize;
            while extracted.iter().any(|(existing, _)| existing == &layer_name) {
                layer_name = format!("{}_{}", base_name, suffix);
                suffix += 1;
            }

            nuke_layers.push(ExportLayer {
                name: layer_name.clone(),
                blend_mode: doc_layer.blend,
                opacity: doc_layer.opacity * doc_layer.fill_opacity,
                visible: doc_layer.visible && parent_visible,
            });
            extracted.push((layer_name, px_data));
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
        let mut flat_data = vec![[0.0_f32; 4]; width * height];
        for (idx, (_, layer_data)) in extracted.iter().enumerate() {
            let layer = &nuke_layers[idx];
            if !layer.visible {
                continue;
            }
            let mode = if idx == 0 { BlendMode::Normal } else { layer.blend_mode };
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