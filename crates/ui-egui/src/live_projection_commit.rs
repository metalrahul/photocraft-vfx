//! A released projected Brush or Eraser stroke is written on a worker thread
//! (`ProjectedStrokeJob`) instead of in the frame of the release: writing a big stroke back into
//! a 16K latlong took 0.1–1 s, and the canvas froze for it. Meanwhile its live preview keeps
//! showing; when the write is done it is offered to `paint.projectedStroke`, which takes the
//! pixels (one History step, the journal as before). Any other command, a new stroke, a moved
//! view or turning the projection off lands it first, waiting for the write if need be.

use std::sync::mpsc::{Receiver, TryRecvError};

use egui::Rect;
use photocraft_engine::brush_cmds::{ProjectedCommit, ProjectedStrokeJob};

use super::{Live, with_brush_scale};
use crate::PhotocraftApp;

type Done = (ProjectedCommit, photocraft_raster::Surface, photocraft_geom::Rect);

/// A stroke being written in the background.
pub struct Pending {
    /// The stroke, with its points in its params, and its preview (shown until it lands).
    live: Live,
    /// The document it paints, and the view it was drawn in (`ViewProjection::key`).
    doc: photocraft_doc::DocId,
    view: u64,
    /// The brush Size factor it was drawn with (`brush_scale`), for the command if it must
    /// paint the stroke itself.
    scale: f32,
    rx: Receiver<Option<Done>>,
}

/// Start writing finished stroke `l` (a `paint.projectedStroke`, points in its params) in the
/// background. Gives it back when it can't (the caller then runs the command as before).
pub(super) fn start(app: &mut PhotocraftApp, l: Live, scale: f32, view: u64) -> Result<(), Live> {
    // One at a time, in order: an earlier stroke lands before this one is prepared on top.
    finish(app, true);
    let Some(doc) = app.session.active().map(|st| st.doc.id) else { return Err(l) };
    let job = match with_brush_scale(app, scale, |app| ProjectedStrokeJob::prepare(&app.session, &l.params)) {
        Ok(job) => job,
        Err(_) => return Err(l),
    };
    let (tx, rx) = std::sync::mpsc::channel();
    let spawned = std::thread::Builder::new()
        .name("pc-projected-commit".into())
        .spawn(move || {
            let (done, before, bounds) = job.run();
            // The changed tiles are converted to the canvas's texels here too, so the frame
            // that lands the stroke only uploads them. (Should the thread die, the channel
            // closes and the command paints the stroke itself.)
            photocraft_gpu::preconvert(done.surface(), bounds, Some(&before));
            let _ = tx.send(Some((done, before, bounds)));
        })
        .is_ok();
    if !spawned {
        return Err(l);
    }
    app.projection_commit = Some(Pending { live: l, doc, view, scale, rx });
    Ok(())
}

/// The canvas of view `idx` (drawn in view `key`): show the stroke being written and land it
/// when it's done (or at once when the view moved, as its preview would be in the wrong place).
pub(super) fn show(app: &mut PhotocraftApp, ctx: &egui::Context, painter: &egui::Painter, rect: Rect, ppp: f32, key: u64) {
    let Some(p) = app.projection_commit.as_mut() else { return };
    if app.session.active().map(|st| st.doc.id) != Some(p.doc) {
        return;
    }
    p.live.draw(ctx, painter, rect, ppp);
    let moved = p.view != key;
    finish(app, moved);
    if app.projection_commit.is_some() {
        ctx.request_repaint_after(std::time::Duration::from_millis(16));
    }
}

/// Land the stroke being written: when its write is done, or waiting for it when `wait`.
pub(crate) fn finish(app: &mut PhotocraftApp, wait: bool) {
    let Some(p) = app.projection_commit.take() else { return };
    let done = if wait {
        let t0 = crate::gpu_canvas::now_ms();
        let r = p.rx.recv().ok().flatten();
        app.perf.stage.wait_ms += crate::gpu_canvas::now_ms() - t0;
        r
    } else {
        match p.rx.try_recv() {
            Ok(r) => r,
            Err(TryRecvError::Empty) => {
                app.projection_commit = Some(p);
                return;
            }
            Err(TryRecvError::Disconnected) => None,
        }
    };
    land(app, p, done);
}

/// Put the stroke into its document: its written pixels when they still fit, painted again
/// by the command otherwise (another edit landed meanwhile, or the write failed).
fn land(app: &mut PhotocraftApp, p: Pending, done: Option<Done>) {
    let Pending { live, doc, scale, .. } = p;
    let Some(index) = app.session.documents().iter().position(|st| st.doc.id == doc) else { return };
    // The document may no longer be the active one (another tab chosen meanwhile).
    let back = app.session.active_index().filter(|i| *i != index);
    if back.is_some() {
        app.session.set_active(index);
    }
    match done {
        Some((done, _, _)) => app.session.offer_projected_stroke(done),
        None => app.session.clear_live_stroke(),
    }
    let r = with_brush_scale(app, scale, |app| app.run(live.cmd, live.params));
    app.session.clear_live_stroke();
    if let Some(i) = back {
        app.session.set_active(i);
    }
    if let Err(e) = r {
        app.ui.status = e;
        app.ui.status_error = true;
    }
}
