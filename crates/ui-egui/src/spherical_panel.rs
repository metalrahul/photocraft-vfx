//! Window › Spherical: a dockable panel (a tab of the Navigator group) that turns a latlong
//! panorama while you keep painting, like Nuke's SphericalTransform rotation.
//!
//! Every spherical control lives here; the main window only turns spherical when **Show in Main
//! Window** is on (or View › Equirectangular Live Projection, its shortcut).
//!
//! It drives the document view's live projection (live_projection.rs): **Latlong** shows the
//! panorama flat, turned by yaw, pitch and roll, so a seam or a pole can be brought to the middle
//! and painted across; **Perspective** is Equirectangular Live Projection's camera. Either way the
//! rotation is a view setting: strokes are written back through it, so the pixels keep their
//! orientation until **Bake Rotation** turns the active layer for real.

use photocraft_engine::equirect::latlong_rotation;
use serde_json::json;

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

    // On / off in the main window, and which view.
    let mut want = on;
    let toggle_sc = crate::shortcuts::shortcut_label(app, "view.equirectLiveProjection");
    let r = widgets::toggle(ui, &mut want, tl!("Show in Main Window"));
    r.on_hover_text(match &toggle_sc {
        Some(sc) => format!("{} ({sc})", tl!("Show the document spherically in the main window")),
        None => tl!("Show the document spherically in the main window").to_string(),
    });
    if want != on {
        let r = crate::live_projection::toggle(app, idx, Some(want));
        if let Err(e) = r {
            app.ui.status = e;
        } else if want && let Some(cam) = app.ui.views.get_mut(idx).and_then(|v| v.projection.as_mut()) {
            // Turned on from this panel: start flat, as the panorama is.
            if !on {
                cam.latlong = true;
            }
        }
    }
    if let Some((w, h)) = size
        && w != 2 * h
    {
        hint(ui, &format!("{} ({w} × {h})", tl!("Spherical tools expect a 2:1 latlong")));
    }
    let Some(mut cam) = app.ui.views.get(idx).and_then(|v| v.projection) else {
        hint(ui, tl!("Turn on Show in Main Window to rotate the panorama while you paint."));
        shortcuts(app, ui, false);
        return;
    };
    let before = cam;

    ui.add_space(4.0);
    ui.horizontal(|ui| {
        if ui.selectable_label(cam.latlong, tl!("Latlong")).clicked() {
            cam.latlong = true;
        }
        if ui.selectable_label(!cam.latlong, tl!("Perspective")).clicked() {
            cam.latlong = false;
        }
    });
    ui.add_space(4.0);

    widgets::slider_row(ui, tl!("Yaw"), &mut cam.yaw, -180.0..=180.0, "°", None);
    let pitch_max = if cam.latlong { 90.0 } else { 89.9 };
    widgets::slider_row(ui, tl!("Pitch"), &mut cam.pitch, -pitch_max..=pitch_max, "°", None);
    ui.add_enabled_ui(cam.latlong, |ui| {
        widgets::slider_row(ui, tl!("Roll"), &mut cam.roll, -180.0..=180.0, "°", None);
    });
    if cam.latlong {
        let mut zoom = if cam.scale > 0.0 { cam.scale * 100.0 } else { 0.0 };
        if widgets::slider_row(ui, tl!("Zoom"), &mut zoom, 0.0..=800.0, "%", None).changed() {
            cam.scale = (zoom / 100.0).max(0.0);
        }
    } else {
        widgets::slider_row(ui, tl!("Field of View"), &mut cam.fov, 5.0..=150.0, "°", None);
    }

    ui.add_space(6.0);
    ui.horizontal(|ui| {
        if widgets::compact_button(ui, tl!("Reset"), 22.0).on_hover_text(tl!("Yaw, pitch and roll back to 0")).clicked() {
            (cam.yaw, cam.pitch, cam.roll) = (0.0, 0.0, 0.0);
            cam.fov = ProjectionView::default().fov;
        }
        if cam.latlong && widgets::compact_button(ui, tl!("Fit"), 22.0).on_hover_text(tl!("Show the whole panorama")).clicked() {
            (cam.scale, cam.center_v) = (0.0, -1.0);
        }
    });
    let rotated = cam.yaw != 0.0 || cam.pitch != 0.0 || cam.roll != 0.0;
    ui.add_space(4.0);
    let bake = ui
        .add_enabled_ui(cam.latlong && rotated, |ui| widgets::compact_button(ui, tl!("Bake Rotation"), 22.0))
        .inner
        .on_hover_text(tl!("Turn the active layer's pixels (or the selected part) by this rotation, then show it unrotated"));

    if cam != before
        && let Some(v) = app.ui.views.get_mut(idx).and_then(|v| v.projection.as_mut())
    {
        *v = cam;
        v.clamp_latlong();
    }
    if bake.clicked() {
        bake_rotation(app, idx, cam);
    }

    ui.add_space(6.0);
    let mut doc_px = app.ui.tool_options.projection_brush_doc_px;
    if widgets::checkbox(ui, &mut doc_px, tl!("Brush Size in Document Pixels"))
        .on_hover_text(tl!("Measure the brush Size in latlong pixels at the view's centre instead of screen pixels"))
        .changed()
    {
        app.ui.tool_options.projection_brush_doc_px = doc_px;
    }

    ui.add_space(8.0);
    hint(ui, tl!("Paint as usual: strokes are written back through the rotation, so the pixels keep their orientation."));
    shortcuts(app, ui, cam.latlong);
}

/// The view's keys and mouse gestures, listed in the panel.
fn shortcuts(app: &PhotocraftApp, ui: &mut egui::Ui, latlong: bool) {
    let t = Tokens::get(ui.ctx());
    let toggle = crate::shortcuts::shortcut_label(app, "view.equirectLiveProjection").unwrap_or_else(|| "—".into());
    let look = if latlong { tl!("Pan (sideways turns the yaw)") } else { tl!("Look around") };
    let zoom = if latlong { tl!("Zoom") } else { tl!("Field of view") };
    let rows: [(String, &str); 8] = [
        (toggle, tl!("Show in main window on / off")),
        (tl!("Space-drag, middle-drag, Hand").into(), look),
        (tl!("Arrow keys").into(), tl!("Look 2° (Shift 10°, Alt 0.5°)")),
        (tl!("Home").into(), tl!("Look straight ahead")),
        (tl!("Wheel, Ctrl + / Ctrl −").into(), zoom),
        (tl!("Zoom tool drag").into(), tl!("Scrubby zoom (right zooms in)")),
        (tl!("Zoom tool click").into(), tl!("Zoom in (Alt-click: out)")),
        (tl!("Alt-click (Clone Stamp)").into(), tl!("Set the source direction")),
    ];
    ui.add_space(6.0);
    egui::CollapsingHeader::new(egui::RichText::new(tl!("Shortcuts")).size(11.5)).id_salt("spherical_shortcuts").default_open(true).show(ui, |ui| {
        egui::Grid::new("spherical_shortcut_grid").num_columns(2).spacing([10.0, 3.0]).show(ui, |ui| {
            for (keys, what) in rows {
                ui.label(egui::RichText::new(keys).size(11.0).color(t.text));
                ui.label(egui::RichText::new(what).size(11.0).color(t.text_faint));
                ui.end_row();
            }
        });
    });
}

/// Bake Rotation: turn the active layer so the document shows what the rotated latlong shows,
/// then reset the view's rotation (the picture stays where it is).
fn bake_rotation(app: &mut PhotocraftApp, idx: usize, cam: ProjectionView) {
    // The view shows document direction `R · d` at view direction `d`; the baked layer must hold
    // that at `d`. layer.projectedRotate's `q` makes pixel `d` sample `qᵀ · d`, so q = Rᵀ.
    let r = latlong_rotation(f64::from(cam.yaw), f64::from(cam.pitch), f64::from(cam.roll));
    let q = [[r[0][0], r[1][0], r[2][0]], [r[0][1], r[1][1], r[2][1]], [r[0][2], r[1][2], r[2][2]]];
    match app.run("layer.projectedRotate", json!({"rotation": q.concat()})) {
        Ok(_) => {
            if let Some(v) = app.ui.views.get_mut(idx).and_then(|v| v.projection.as_mut()) {
                (v.yaw, v.pitch, v.roll) = (0.0, 0.0, 0.0);
            }
            app.ui.status = tl!("Rotation baked into the layer").into();
        }
        Err(e) => app.ui.status = e,
    }
}
