use exr::prelude::*;
use photocraft_doc::{Document, Layer as DocLayer, LayerContent};
use std::path::Path;

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

pub fn export_multichannel_exr_with_options(
    doc: &Document,
    path: &Path,
    compression: NukeCompression,
    include_flat_beauty: bool,
) -> std::result::Result<(), String> {
    let width = doc.size.width as usize;
    let height = doc.size.height as usize;

    if width == 0 || height == 0 {
        return Err("Document dimensions must be greater than zero.".into());
    }

    let bounds = doc.bounds();
    let mut extracted: Vec<(String, Vec<[f32; 4]>)> = Vec::new();

    // 1. Traversal loop for all visual layer frames in the stack
    let mut stack: Vec<&DocLayer> = doc.layers.iter().rev().collect();
    while let Some(doc_layer) = stack.pop() {
        if let LayerContent::Raster(surface) = &doc_layer.content {
            let px_data = photocraft_compose::surface_to_buffer(surface, bounds).px;

            let base_name = if doc_layer.name.trim().is_empty() {
                format!("Layer_{}", extracted.len() + 1)
            } else {
                doc_layer.name.replace(' ', "_")
            };

            let mut layer_name = base_name.clone();
            let mut suffix = 2usize;
            while extracted.iter().any(|(existing, _)| existing == &layer_name) {
                layer_name = format!("{}_{}", base_name, suffix);
                suffix += 1;
            }

            extracted.push((layer_name, px_data));
        }

        if let Some(children) = doc_layer.children() {
            stack.extend(children.iter().rev());
        }
    }

    if extracted.is_empty() {
        return Err("No layers available for export.".into());
    }

    // 2. Generate manual flat beauty pass by alpha-compositing all extracted layers bottom-to-top
    if include_flat_beauty {
        let mut flat_data = vec![[0.0_f32; 4]; width * height];
        for (_, layer_data) in &extracted {
            for i in 0..(width * height) {
                let src = layer_data[i];
                let dst = flat_data[i];

                let src_a = src[3];
                let dst_a = dst[3];
                
                // Calculate over-blend alpha
                let out_a = src_a + dst_a * (1.0 - src_a);

                if out_a > 0.0 {
                    // Straight alpha over-blend
                    flat_data[i][0] = (src[0] * src_a + dst[0] * dst_a * (1.0 - src_a)) / out_a;
                    flat_data[i][1] = (src[1] * src_a + dst[1] * dst_a * (1.0 - src_a)) / out_a;
                    flat_data[i][2] = (src[2] * src_a + dst[2] * dst_a * (1.0 - src_a)) / out_a;
                    flat_data[i][3] = out_a;
                }
            }
        }
        // Push as "rgba" so it gets caught by the Nuke channel namer
        extracted.push(("rgba".to_string(), flat_data));
    }

    let exr_compression = compression.to_exr_compression();

    // 3. Build the OpenEXR file
    let exr_layers: Layers<_> = extracted
        .into_iter()
        .map(|(name, data)| {
            let n = name.clone();
            
            // Nuke specifically looks for "R", "G", "B", "A" channels at the root level to populate the 'rgba' viewer
            let (r_name, g_name, b_name, a_name) = if n == "rgba" {
                ("R".to_string(), "G".to_string(), "B".to_string(), "A".to_string())
            } else {
                (format!("{}.R", n), format!("{}.G", n), format!("{}.B", n), format!("{}.A", n))
            };

            let channels = SpecificChannels::new(
                (
                    ChannelDescription::named(r_name.as_str(), SampleType::F32),
                    ChannelDescription::named(g_name.as_str(), SampleType::F32),
                    ChannelDescription::named(b_name.as_str(), SampleType::F32),
                    ChannelDescription::named(a_name.as_str(), SampleType::F32),
                ),
                move |pos: Vec2<usize>| {
                    let p = data
                        .get(pos.y() * width + pos.x())
                        .copied()
                        .unwrap_or([0.0, 0.0, 0.0, 0.0]);
                    (p[0], p[1], p[2], p[3])
                },
            );

            Layer::new(
                Vec2(width, height),
                // Keep the part name empty for the root beauty pass so it drops straight into Nuke's global space
                LayerAttributes::named(if n == "rgba" { "" } else { name.as_str() }),
                Encoding {
                    compression: exr_compression,
                    line_order: LineOrder::Increasing,
                    blocks: Blocks::ScanLines,
                },
                channels,
            )
        })
        .collect();

    let attributes = ImageAttributes::new(IntegerBounds::from_dimensions(Vec2(width, height)));
    let image = Image::from_layers(attributes, exr_layers);

    image.write().to_file(path).map_err(|e| e.to_string())?;

    Ok(())
}

pub fn export_multichannel_exr(doc: &Document, path: &Path) -> std::result::Result<(), String> {
    export_multichannel_exr_with_options(doc, path, NukeCompression::ZipScanline, true)
}