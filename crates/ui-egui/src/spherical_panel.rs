//! Window › Spherical: a dockable panel (a tab of the Navigator group) that turns a latlong
//! panorama while you keep painting, like Nuke's SphericalTransform rotation.
//!
//! Every spherical control lives here; the main window only turns spherical when **Enable
//! Spherical Projection** is on (or View › Equirectangular Live Projection, its shortcut). The
//! Navigator stays the ordinary 2D navigator.
//!
//! It drives the document view's live projection (live_projection.rs): **Latlong** shows the
//! panorama flat, turned by yaw, pitch and roll, so a seam or a pole can be brought to the middle
//! and painted across; **Perspective** is Equirectangular Live Projection's camera. Either way the
//! rotation is a view setting: strokes are written back through it, so the pixels keep their
//! orientation.

use crate::PhotocraftApp;
use crate::live_projection::ProjectionView;
use crate::theme::Tokens;
use crate::widgets;

/// The panel body.
pub(crate) fn panel(app: &mut PhotocraftApp, ui: &mut egui::Ui) {
    let t = Tokens::get(ui.ctx());
    let hint = |ui: &mut egui::Ui, s: &str| {
        ui.label(egui::RichText::new(s).color(t.text_faint).size(11.0));
    };
    let Some(idx) = app.session.active_index() else {
        hint(ui, tl!("No document"));
        return;
    };
    let size = app.session.documents().get(idx).map(|d| (d.doc.size.width, d.doc.size.height));
    let on = crate::live_projection::active(app, idx);

    // On / off: the main window shows the document spherically.
    let mut want = on;
    let tip = match crate::shortcuts::shortcut_label(app, "view.equirectLiveProjection") {
        Some(sc) => format!("{} ({sc})", tl!("Show the document spherically in the main window")),
        None => tl!("Show the document spherically in the main window").to_string(),
    };
    widgets::toggle(ui, &mut want, tl!("Enable Spherical Projection")).on_hover_text(tip);
    if want != on {
        match crate::live_projection::toggle(app, idx, Some(want)) {
            Err(e) => app.ui.status = e,
            // Turned on from this panel: start flat, as the panorama is.
            Ok(true) if !on => {
                if let Some(cam) = app.ui.views.get_mut(idx).and_then(|v| v.projection.as_mut()) {
                    cam.latlong = true;
                }
            }
            Ok(_) => {}
        }
    }
    if let Some((w, h)) = size
        && w != 2 * h
    {
        hint(ui, &format!("{} ({w} × {h})", tl!("Spherical tools expect a 2:1 latlong")));
    }
    let Some(actual) = app.ui.views.get(idx).and_then(|v| v.projection) else {
        shortcuts(app, ui, false);
        return;
    };

    // Live Update off: a drag on the map or a slider changes a pending camera, drawn on the map,
    // and the main window turns to it on release (no full redraw of a large panorama per step).
    let ctx = ui.ctx().clone();
    let pending_id = egui::Id::new(("pc-spherical-pending", idx));
    let live = app.ui.tool_options.projection_live_update;
    let mut cam = if live { actual } else { ctx.data(|d| d.get_temp::<ProjectionView>(pending_id)).unwrap_or(actual) };

    // The latlong map: where the view looks; click or drag to point it, the wheel zooms.
    ui.add_space(6.0);
    map(app, ui, idx, &mut cam);
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        if ui.selectable_label(cam.latlong, tl!("Latlong")).clicked() {
            cam.latlong = true;
        }
        if ui.selectable_label(!cam.latlong, tl!("Perspective")).clicked() {
            cam.latlong = false;
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if cam.latlong && widgets::compact_button(ui, tl!("Fit"), 20.0).on_hover_text(tl!("Show the whole panorama")).clicked() {
                (cam.scale, cam.center_v) = (0.0, -1.0);
            }
            if widgets::compact_button(ui, tl!("Reset"), 20.0).on_hover_text(tl!("Yaw, pitch and roll back to 0")).clicked() {
                (cam.yaw, cam.pitch, cam.roll) = (0.0, 0.0, 0.0);
                cam.fov = ProjectionView::default().fov;
            }
        });
    });
    ui.add_space(4.0);

    widgets::slider_row(ui, tl!("Yaw"), &mut cam.yaw, -180.0..=180.0, "°", None);
    let pitch_max = if cam.latlong { 90.0 } else { 89.9 };
    widgets::slider_row(ui, tl!("Pitch"), &mut cam.pitch, -pitch_max..=pitch_max, "°", None);
    if cam.latlong {
        widgets::slider_row(ui, tl!("Roll"), &mut cam.roll, -180.0..=180.0, "°", None);
        let mut zoom = if cam.scale > 0.0 { cam.scale * 100.0 } else { 0.0 };
        if widgets::slider_row(ui, tl!("Zoom"), &mut zoom, 0.0..=800.0, "%", None).changed() {
            cam.scale = (zoom / 100.0).max(0.0);
        }
    } else {
        widgets::slider_row(ui, tl!("Field of View"), &mut cam.fov, 5.0..=150.0, "°", None);
    }

    if !live && ctx.input(|i| i.pointer.any_down()) && cam != actual {
        // Still dragging: keep the change pending.
        ctx.data_mut(|d| d.insert_temp(pending_id, cam));
    } else {
        ctx.data_mut(|d| d.remove_temp::<ProjectionView>(pending_id));
        if cam != actual
            && let Some(v) = app.ui.views.get_mut(idx).and_then(|v| v.projection.as_mut())
        {
            *v = cam;
            v.clamp_latlong();
        }
    }

    ui.add_space(6.0);
    egui::CollapsingHeader::new(egui::RichText::new(tl!("Options")).size(11.5)).id_salt("spherical_options").default_open(false).show(ui, |ui| {
        let mut live = live;
        if widgets::checkbox(ui, &mut live, tl!("Live Update"))
            .on_hover_text(tl!("Turn the main window while you drag; off, it turns when you release"))
            .changed()
        {
            app.ui.tool_options.projection_live_update = live;
        }
        let mut doc_px = app.ui.tool_options.projection_brush_doc_px;
        if widgets::checkbox(ui, &mut doc_px, tl!("Brush Size in Document Pixels"))
            .on_hover_text(tl!("Measure the brush Size in latlong pixels at the view's centre instead of screen pixels"))
            .changed()
        {
            app.ui.tool_options.projection_brush_doc_px = doc_px;
        }
    });
    shortcuts(app, ui, cam.latlong);
}

/// The document as a latlong with `cam`'s outline, like the Navigator's preview; clicking or
/// dragging points `cam` (`live_projection_misc::navigator`).
fn map(app: &mut PhotocraftApp, ui: &mut egui::Ui, idx: usize, cam: &mut ProjectionView) {
    let t = Tokens::get(ui.ctx());
    let ctx = ui.ctx().clone();
    let Some(size) = app.session.documents().get(idx).map(|d| d.doc.size) else { return };
    // The panorama's own aspect, at most 220 points tall: the GPU preview is drawn at the rect's
    // width, so a rect shorter than the image would let it spill over the controls around it.
    let aspect = size.height.max(1) as f32 / size.width.max(1) as f32;
    let full = ui.available_width().max(60.0);
    let h = (full * aspect).min(220.0).max(20.0);
    let w = (h / aspect).min(full);
    let (row, _) = ui.allocate_exact_size(egui::vec2(full, h), egui::Sense::hover());
    let rect = egui::Rect::from_center_size(row.center(), egui::vec2(w, w * aspect));
    let resp = ui.interact(rect, ui.id().with("pc-spherical-map"), egui::Sense::click_and_drag());
    ui.painter().rect_filled(rect, t.radius_sm, t.canvas);
    if let Some(params) = crate::canvas::navigator_gpu_view(app, idx, rect, &ctx) {
        crate::gpu_canvas::GpuCanvas::paint(ui.painter(), rect, params);
    } else if !crate::canvas::navigator_waits_for_gpu(app, idx, &ctx)
        && let Some(tex) = crate::canvas::navigator_texture(app, &ctx, idx)
    {
        ui.painter().image(tex, rect, egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)), egui::Color32::WHITE);
    }
    crate::live_projection_misc::navigator(app, ui, &ctx, &resp, idx, rect, cam);
    resp.on_hover_text(tl!("Click or drag to point the view; the wheel zooms"));
}

/// The view's keys and mouse gestures, listed in the panel.
fn shortcuts(app: &PhotocraftApp, ui: &mut egui::Ui, latlong: bool) {
    let t = Tokens::get(ui.ctx());
    let toggle = crate::shortcuts::shortcut_label(app, "view.equirectLiveProjection").unwrap_or_else(|| "—".into());
    let look = if latlong { tl!("Pan (sideways turns the yaw)") } else { tl!("Look around") };
    let zoom = if latlong { tl!("Zoom") } else { tl!("Field of view") };
    let rows: [(String, &str); 8] = [
        (toggle, tl!("Spherical projection on / off")),
        (tl!("Space-drag, middle-drag, Hand").into(), look),
        (tl!("Arrow keys").into(), tl!("Look 2° (Shift 10°, Alt 0.5°)")),
        (tl!("Home").into(), tl!("Look straight ahead")),
        (tl!("Wheel, Ctrl + / Ctrl −").into(), zoom),
        (tl!("Zoom tool drag").into(), tl!("Scrubby zoom (right zooms in)")),
        (tl!("Zoom tool click").into(), tl!("Zoom in (Alt-click: out)")),
        (tl!("Alt-click (Clone Stamp)").into(), tl!("Set the source direction")),
    ];
    ui.add_space(6.0);
    egui::CollapsingHeader::new(egui::RichText::new(tl!("Shortcuts")).size(11.5)).id_salt("spherical_shortcuts").default_open(false).show(ui, |ui| {
        egui::Grid::new("spherical_shortcut_grid").num_columns(2).spacing([10.0, 3.0]).show(ui, |ui| {
            for (keys, what) in rows {
                ui.label(egui::RichText::new(keys).size(11.0).color(t.text));
                ui.label(egui::RichText::new(what).size(11.0).color(t.text_faint));
                ui.end_row();
            }
        });
    });
}
