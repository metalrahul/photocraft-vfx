//! `paint.projectedClone`: the Clone Stamp in View › Equirectangular Live Projection.
//!
//! The source is a *rotation of the sphere* (`"rotation"`, row-major 3×3): destination direction
//! `d` samples the image at direction `rotation · d`. Both sides are resampled into the view the
//! stroke is drawn in, so the clone looks undistorted there, the ordinary Clone Stamp coverage and
//! blend run in view space, and the change is written back onto the equirect pixels as the
//! projected Brush does (`brush_cmds::projected`). One History step per stroke.

use photocraft_geom::Rect;
use photocraft_paint::retouch::{Region, apply_coverage, stroke_coverage};
use photocraft_raster::Surface;
use serde_json::{Value, json};

use super::{SampleLayers, bad, blend_param, composite_region, damage_json, parse_brush, run_stroke, sample_layers};
use crate::brush_cmds::projected::to_view;
use crate::equirect::{ViewProjection, rot_from_slice, view_rect_to_equirect};
use crate::{Result, Session};

const CMD: &str = "paint.projectedClone";

/// What a `paint.projectedClone` needs from its params: the view, the source view, the stroke,
/// its layer, which layers it samples and the blend mode.
struct Parsed {
    proj: ViewProjection,
    src_proj: ViewProjection,
    stroke: photocraft_paint::Stroke,
    id: Option<photocraft_doc::LayerId>,
    which: SampleLayers,
    mode: photocraft_color::BlendMode,
}

fn parse(s: &Session, p: &Value) -> Result<Parsed> {
    let q = p.get("projection").ok_or_else(|| bad(CMD, "missing `projection`"))?;
    let g = |k: &str, d: f64| q.get(k).and_then(Value::as_f64).unwrap_or(d);
    let (w, h) = (g("width", 0.0), g("height", 0.0));
    if !(w >= 1.0 && h >= 1.0 && w <= 16384.0 && h <= 16384.0) {
        return Err(bad(CMD, "`projection.width` and `height` must be 1..=16384"));
    }
    let proj = ViewProjection::from_json(q, w, h);
    let rotation: Vec<f64> = p.get("rotation").and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_f64).collect()).unwrap_or_default();
    let rotation = rot_from_slice(&rotation).ok_or_else(|| bad(CMD, "`rotation` must be a 3×3 rotation (9 numbers, row-major)"))?;
    let src_proj = proj.rotated(&rotation);
    let (stroke, id) = parse_brush(s, p, CMD)?;
    let which = sample_layers(p, CMD)?;
    let mode = blend_param(p, CMD)?;
    Ok(Parsed { proj, src_proj, stroke, id, which, mode })
}

pub(super) fn projected_clone(s: &mut Session, p: &Value) -> Result<Value> {
    let c = parse(s, p)?;
    // The stroke as the canvas already wrote it on a worker thread, for exactly these params
    // and this document (`clone_job`): its pixels, instead of cloning again.
    let live = s.live_commit.take().filter(|l| l.matches(s, CMD, p, &c.stroke.points));
    let dmg = run_stroke(s, "Clone Stamp", c.id, p, |pre, surf, sel, lock| {
        if let Some(l) = live {
            *surf = l.surface;
            return Ok(l.bounds);
        }
        clone_into(&c, pre, surf, sel, lock)
    })?;
    Ok(json!({ "damage": damage_json(dmg) }))
}

/// A released `paint.projectedClone` (params `p`, with its points) to write on a worker thread
/// while the canvas carries on; its result is offered back to the command.
pub fn clone_job(s: &Session, p: &Value) -> Result<crate::brush_cmds::ProjectedStrokeJob> {
    let c = parse(s, p)?;
    let st = s.active().ok_or(crate::EngineError::NoDocument)?;
    let pre = (*st.doc).clone();
    let mut doc = pre.clone();
    let (surf, lock) = crate::channel_cmds::target_surface(&mut doc, c.id, p)?;
    let surface = surf.clone();
    let points = c.stroke.points.clone();
    let write = Box::new(move |surf: &mut Surface| clone_into(&c, &pre, surf, pre.selection.as_ref(), lock));
    crate::brush_cmds::ProjectedStrokeJob::new(s, CMD, p, points, surface, write)
}

/// Clone stroke `c` onto `surf` (the target in document `pre`), clipped by `sel`.
///
/// Written per document pixel at full size: each pixel under the stroke takes the stroke's
/// coverage at its view position and the source pixel its direction turns to. (Adding the
/// view-space change back, as the Brush does, kept the high-frequency detail of what was there
/// whenever the view showed the panorama smaller than its pixels: the old content ghosted
/// through the clone as speckle.)
fn clone_into(c: &Parsed, pre: &photocraft_doc::Document, surf: &mut Surface, sel: Option<&Surface>, lock: bool) -> Result<Rect> {
    use crate::brush_cmds::projected::{sample_buf, sample_wrapped};
    use rayon::prelude::*;
    let Parsed { proj, src_proj, stroke, id, which, mode } = c;
    let (proj, src_proj, which, id, mode) = (*proj, *src_proj, *which, *id, *mode);
    let (iw, ih) = (pre.size.width as i32, pre.size.height as i32);
    if iw < 2 || ih < 1 {
        return Err(bad(CMD, "document too small for a projected stroke"));
    }
    let (bounds, cov) = stroke_coverage(stroke);
    let view = Rect::new(0, 0, proj.w as i32, proj.h as i32);
    let work = bounds.intersect(&view);
    if work.is_empty() {
        return Ok(Rect::EMPTY);
    }
    // Coverage only over the part of the stroke inside the view.
    let cov = crop(&cov, bounds, work);
    let n = surf.channels();
    let fmt = surf.format();
    let r = [f64::from(work.x0), f64::from(work.y0), f64::from(work.x1), f64::from(work.y1)];
    // What the clone reads, at full size: the target before the stroke, or the composite of
    // the sampled layers where the rotated view looks.
    let source = match which {
        SampleLayers::Current => surf.clone(),
        _ => {
            let mut flat = Surface::new(fmt);
            for e in view_rect_to_equirect(&src_proj, r, iw as u32, ih as u32, 2.0) {
                let rect = Rect::new(e[0] as i32, e[1] as i32, e[2] as i32, e[3] as i32);
                if !rect.is_empty() {
                    flat.write_region(rect, &composite_region(pre, id, which, rect, fmt).data);
                }
            }
            flat
        }
    };
    let (fw, fh) = (f64::from(iw), f64::from(ih));
    let mut total = Rect::EMPTY;
    for e in view_rect_to_equirect(&proj, r, iw as u32, ih as u32, 1.0) {
        let rect = Rect::new(e[0] as i32, e[1] as i32, e[2] as i32, e[3] as i32);
        if rect.is_empty() {
            continue;
        }
        let w = rect.width() as usize;
        let mut cov_doc = vec![0f32; w * rect.height() as usize];
        let mut paint = Region::new(rect, n);
        // Rows in parallel: the coverage under each pixel and the source pixel it clones.
        cov_doc.par_chunks_mut(w).zip(paint.data.par_chunks_mut(w * n)).enumerate().for_each(|(j, (crow, prow))| {
            let y = f64::from(rect.y0 + j as i32) + 0.5;
            let (mut px, mut tmp, mut k) = (vec![0f32; n], vec![0f32; n], [0f32]);
            for (i, cv) in crow.iter_mut().enumerate() {
                let x = f64::from(rect.x0 + i as i32) + 0.5;
                let Some([vx, vy]) = proj.equirect_to_view(x, y, fw, fh) else { continue };
                if !(vx >= r[0] && vx < r[2] && vy >= r[1] && vy < r[3]) {
                    continue;
                }
                sample_buf(&cov, work, 1, vx, vy, &mut k);
                if k[0] <= 0.0 {
                    continue;
                }
                *cv = k[0];
                let [u, v] = src_proj.view_to_equirect(vx, vy, fw, fh);
                sample_wrapped(&source, u, v, iw, ih, &mut px, &mut tmp);
                if let Some(out) = prow.get_mut(i * n..(i + 1) * n) {
                    out.copy_from_slice(&px);
                }
            }
        });
        if cov_doc.iter().all(|c| *c <= 0.0) {
            continue;
        }
        let d = apply_coverage(surf, rect, &cov_doc, stroke.brush.opacity, sel, lock, &paint, mode);
        total = if total.is_empty() { d } else { total.union(&d) };
    }
    Ok(total)
}

/// A live Clone Stamp preview from `paint.projectedClone` params (points unused): the target
/// and the rotated source resampled into the whole view once.
pub fn projected_clone_preview(s: &Session, p: &Value) -> Result<crate::brush_cmds::projected::ProjectedPreview> {
    use crate::brush_cmds::projected::{PreviewKind, ProjectedPreview};
    let q = p.get("projection").ok_or_else(|| bad(CMD, "missing `projection`"))?;
    let g = |k: &str, d: f64| q.get(k).and_then(Value::as_f64).unwrap_or(d);
    let (w, h) = (g("width", 0.0), g("height", 0.0));
    if !(w >= 1.0 && h >= 1.0 && w <= 16384.0 && h <= 16384.0) {
        return Err(bad(CMD, "`projection.width` and `height` must be 1..=16384"));
    }
    let proj = ViewProjection::from_json(q, w, h);
    let rotation: Vec<f64> = p.get("rotation").and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_f64).collect()).unwrap_or_default();
    let rotation = rot_from_slice(&rotation).ok_or_else(|| bad(CMD, "`rotation` must be a 3×3 rotation"))?;
    let src_proj = proj.rotated(&rotation);
    let mut pp = p.clone();
    pp["points"] = json!([[0, 0]]);
    let (stroke, id) = parse_brush(s, &pp, CMD)?;
    let which = sample_layers(p, CMD)?;
    let mode = blend_param(p, CMD)?;
    let st = s.active().ok_or(crate::EngineError::NoDocument)?;
    let pre = st.doc.clone();
    let mut doc = (*pre).clone();
    let (iw, ih) = (doc.size.width as i32, doc.size.height as i32);
    let (surf, lock) = crate::channel_cmds::target_surface(&mut doc, id, p)?;
    let view = Rect::new(0, 0, w as i32, h as i32);
    let source = match which {
        SampleLayers::Current => to_view(surf, &src_proj, view, iw, ih),
        _ => {
            // The composite of what the rotated view looks at. A zoomed-out view shows the
            // panorama at a fraction of its pixels: composite a proxy reduced to about the
            // view's resolution, not the full document (a whole 16K latlong took about 2 s at
            // the click). The commit composites at full size where the stroke is.
            let k = preview_reduction(&proj, f64::from(iw));
            let proxy = (k > 1 && photocraft_compose::proxy::proxy_faithful(&pre)).then(|| photocraft_compose::proxy::proxy_document(&pre, k));
            let from = proxy.as_ref().unwrap_or(&pre);
            let (pw, ph) = (from.size.width as i32, from.size.height as i32);
            let fmt = surf.format();
            let mut flat = Surface::new(fmt);
            for e in view_rect_to_equirect(&src_proj, [0.0, 0.0, w, h], pw as u32, ph as u32, 2.0) {
                let rect = Rect::new(e[0] as i32, e[1] as i32, e[2] as i32, e[3] as i32);
                if !rect.is_empty() {
                    flat.write_region(rect, &composite_region(from, id, which, rect, fmt).data);
                }
            }
            to_view(&flat, &src_proj, view, pw, ph)
        }
    };
    let pre_view = to_view(surf, &proj, view, iw, ih);
    let sel = pre.selection.as_ref().map(|m| to_view(m, &proj, view, iw, ih));
    Ok(ProjectedPreview::with(pre_view, view, sel, PreviewKind::Clone { source, brush: Box::new(stroke.brush), mode, lock }))
}

/// The power-of-two factor a document `iw` pixels wide can be reduced by and still have about a
/// pixel per pixel of view `proj` (at its centre, for the perspective camera).
fn preview_reduction(proj: &ViewProjection, iw: f64) -> u32 {
    let per_view_px = match proj.latlong_rect() {
        Some([_, _, pw, _]) => iw / pw.max(1.0),
        None => (proj.cam.fov.to_radians() / 2.0).tan() / (proj.h / 2.0).max(1.0) * iw / std::f64::consts::TAU,
    };
    let mut k = 1u32;
    while f64::from(k * 2) <= per_view_px && k < 64 {
        k *= 2;
    }
    k
}

/// `cov` (row-major over `from`) cut to `to` (inside `from`).
fn crop(cov: &[f32], from: Rect, to: Rect) -> Vec<f32> {
    if from == to {
        return cov.to_vec();
    }
    let fw = from.width() as usize;
    let mut out = Vec::with_capacity(to.width() as usize * to.height() as usize);
    for y in to.y0..to.y1 {
        let row = (y - from.y0) as usize * fw;
        out.extend_from_slice(&cov[row + (to.x0 - from.x0) as usize..row + (to.x1 - from.x0) as usize]);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::equirect::{Camera, rot, rotation_between};

    #[test]
    fn the_preview_composites_at_about_the_views_resolution() {
        // A 16K latlong fitted into a 2000 px view: 8 document pixels per view pixel.
        let fit = ViewProjection::latlong(0.0, 0.0, 0.0, 2000.0, 1200.0, 16384.0, 8192.0, 2000.0 / 16384.0, 4096.0);
        assert_eq!(preview_reduction(&fit, 16384.0), 8);
        // Zoomed in to 1:1: nothing to reduce.
        let close = ViewProjection::latlong(0.0, 0.0, 0.0, 2000.0, 1200.0, 16384.0, 8192.0, 1.0, 4096.0);
        assert_eq!(preview_reduction(&close, 16384.0), 1);
        // A 90° perspective view 1000 px tall of a 16K latlong: about 5 px each, so 4.
        let persp = ViewProjection::new(Camera { yaw: 0.0, pitch: 0.0, fov: 90.0 }, 1600.0, 1000.0);
        assert_eq!(preview_reduction(&persp, 16384.0), 4);
    }

    fn rgba(s: &Session, x: i32, y: i32) -> [f32; 4] {
        let d = s.active().unwrap();
        d.doc.layer(d.active_layer.unwrap()).unwrap().surface().unwrap().rgba(x, y)
    }

    fn doc() -> Session {
        let mut s = Session::new();
        s.execute("file.new", json!({"width": 720, "height": 360, "background": "transparent"})).unwrap();
        s.execute("layer.new.layer", json!({"name": "paint"})).unwrap();
        s
    }

    fn view(yaw: f64) -> Value {
        json!({"yaw": yaw, "pitch": 0.0, "fov": 90.0, "width": 400, "height": 300})
    }

    fn clone(s: &mut Session, yaw: f64, at: [f64; 2], q: &crate::equirect::Rot) {
        s.execute(CMD, json!({"points": [at], "size": 30, "hardness": 100, "rotation": q.concat(), "projection": view(yaw)})).unwrap();
    }

    #[test]
    fn clones_a_patch_across_the_seam() {
        let mut s = doc();
        // A red patch around longitude 0 (the image centre, u = 360).
        s.execute("select.rect", json!({"x": 340, "y": 160, "width": 40, "height": 40})).unwrap();
        s.execute("edit.fill", json!({"color": "#ff0000"})).unwrap();
        s.execute("select.deselect", json!({})).unwrap();
        // Looking at 180° (the seam), clone from the patch: the source is half a turn round.
        let p = ViewProjection::new(Camera { yaw: 180.0, pitch: 0.0, fov: 90.0 }, 400.0, 300.0);
        let dest = p.view_to_dir(200.0, 150.0);
        let src = crate::equirect::equirect_to_dir(360.0, 180.0, 720.0, 360.0);
        let q = rotation_between(dest, src);
        clone(&mut s, 180.0, [200.0, 150.0], &q);
        for x in [1, 718] {
            let c = rgba(&s, x, 180);
            assert!(c[0] > 0.9 && c[3] > 0.9, "x {x}: {c:?}");
        }
        assert_eq!(rgba(&s, 200, 180)[3], 0.0, "nothing else changes");
        // One History step.
        s.execute("edit.undo", json!({})).unwrap();
        assert_eq!(rgba(&s, 1, 180)[3], 0.0);
    }

    #[test]
    fn a_clone_in_a_zoomed_out_view_leaves_none_of_what_was_there() {
        // A one-pixel checker everywhere but a red source square, seen at a quarter size.
        let mut s = doc();
        let id = s.active().unwrap().active_layer.unwrap();
        s.edit("checker", |doc, _| {
            let surf = doc.layer_mut(id).unwrap().surface_mut().unwrap();
            let mut d = vec![0f32; 720 * 360 * 4];
            for y in 0..360 {
                for x in 0..720 {
                    let px = if (330..390).contains(&x) && (150..210).contains(&y) {
                        [1.0, 0.0, 0.0, 1.0]
                    } else if (x + y) % 2 == 0 {
                        [0.0, 0.0, 0.0, 1.0]
                    } else {
                        [1.0, 1.0, 1.0, 1.0]
                    };
                    d[(y * 720 + x) * 4..][..4].copy_from_slice(&px);
                }
            }
            surf.write_region(Rect::new(0, 0, 720, 360), &d);
            Ok(())
        })
        .unwrap();
        let p = ViewProjection::latlong(0.0, 0.0, 0.0, 180.0, 90.0, 720.0, 360.0, 0.25, 180.0);
        // Clone the red square (view 90, 45) onto the checker 30° to the right (view 105, 45).
        let q = rotation_between(p.view_to_dir(105.0, 45.0), p.view_to_dir(90.0, 45.0));
        let view = json!({"kind": "latlong", "yaw": 0.0, "scale": 0.25, "centerY": 180.0, "docWidth": 720, "docHeight": 360, "width": 180, "height": 90});
        s.execute(CMD, json!({"points": [[105.0, 45.0]], "size": 10, "hardness": 100, "rotation": q.concat(), "projection": view})).unwrap();
        // The middle of the clone (doc 420, 180) is the source's red, checker gone.
        for (x, y) in [(420, 180), (421, 180), (420, 181), (415, 176)] {
            let c = rgba(&s, x, y);
            assert!(c[0] > 0.99 && c[1] < 0.01 && c[2] < 0.01, "({x}, {y}): {c:?}");
        }
    }

    #[test]
    fn a_clone_written_on_a_worker_thread_lands_as_the_command_would_paint_it() {
        let setup = || {
            let mut s = doc();
            s.execute("select.rect", json!({"x": 340, "y": 160, "width": 40, "height": 40})).unwrap();
            s.execute("edit.fill", json!({"color": "#ff0000"})).unwrap();
            s.execute("select.deselect", json!({})).unwrap();
            s
        };
        let p = ViewProjection::new(Camera { yaw: 180.0, pitch: 0.0, fov: 90.0 }, 400.0, 300.0);
        let q = rotation_between(p.view_to_dir(200.0, 150.0), crate::equirect::equirect_to_dir(360.0, 180.0, 720.0, 360.0));
        let params = json!({"points": [[200.0, 150.0]], "size": 30, "hardness": 100, "rotation": q.concat(), "sampleLayer": "all", "projection": view(180.0)});
        let mut direct = setup();
        direct.execute(CMD, params.clone()).unwrap();
        let mut s = setup();
        let job = clone_job(&s, &params).unwrap();
        let (done, _, bounds) = std::thread::spawn(move || job.run()).join().unwrap().unwrap();
        assert!(!bounds.is_empty());
        s.offer_projected_stroke(done);
        s.execute(CMD, params).unwrap();
        assert!(s.live_commit.is_none(), "the offer is used up");
        for x in [1, 718, 200, 360] {
            assert_eq!(rgba(&s, x, 180), rgba(&direct, x, 180), "x {x}");
        }
        assert!(rgba(&s, 1, 180)[0] > 0.9);
        s.execute("edit.undo", json!({})).unwrap();
        assert_eq!(rgba(&s, 1, 180)[3], 0.0, "one History step");
    }

    #[test]
    fn aligned_rotation_survives_a_yaw_change() {
        let mut s = doc();
        // Green at longitude +90° (u = 540); the source rotation is "90° to the right of here".
        s.execute("select.rect", json!({"x": 520, "y": 160, "width": 40, "height": 40})).unwrap();
        s.execute("edit.fill", json!({"color": "#00ff00"})).unwrap();
        s.execute("select.deselect", json!({})).unwrap();
        let a = ViewProjection::new(Camera { yaw: 0.0, pitch: 0.0, fov: 90.0 }, 400.0, 300.0);
        let q = rotation_between(a.view_to_dir(200.0, 150.0), crate::equirect::equirect_to_dir(540.0, 180.0, 720.0, 360.0));
        // Stroke 1 at yaw 0, centre (u = 360) → green.
        clone(&mut s, 0.0, [200.0, 150.0], &q);
        assert!(rgba(&s, 360, 180)[1] > 0.9);
        // Look 20° to the left and paint at another sphere point (right of centre): the
        // same rotation still samples the green patch.
        let b = ViewProjection::new(Camera { yaw: -20.0, pitch: 0.0, fov: 90.0 }, 400.0, 300.0);
        let d = crate::equirect::equirect_to_dir(370.0, 180.0, 720.0, 360.0);
        let at = b.dir_to_view(d).unwrap();
        clone(&mut s, -20.0, at, &q);
        let c = rgba(&s, 370, 180);
        assert!(c[1] > 0.9 && c[3] > 0.9, "{c:?}");
        let m = rot(&q, d);
        let [u, _] = crate::equirect::dir_to_equirect(m, 720.0, 360.0);
        assert!((u - 550.0).abs() < 1.0, "the source moved with the stroke: {u}");
    }
}
