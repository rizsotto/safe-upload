//! Headless UI tests for the editor widget. Run under a private weston,
//! D-Bus and home (see `common/headless.rs`); `cargo test` needs no setup.

#[path = "common/headless.rs"]
mod headless;

use std::cell::Cell;
use std::process::ExitCode;
use std::rc::Rc;
use std::time::Duration;

use gtk::prelude::*;
use gtk::{gdk, graphene};
use image::{Rgba, RgbaImage};
use safe_upload_editor_widget::{AreaKind, EditModel, Rect, RedactionEditor, Tool};

const WATCHDOG: Duration = Duration::from_secs(120);
const GRAY: u8 = 200;

/// An editor in a presented window, showing a gray preview of `preview`
/// pixels for an image of `full` pixels.
fn editor(
    full: (u32, u32),
    preview: (u32, u32),
    model: Option<EditModel>,
) -> (gtk::Window, RedactionEditor) {
    let ed = RedactionEditor::new();
    let img = RgbaImage::from_pixel(preview.0, preview.1, Rgba([GRAY, GRAY, GRAY, 255]));
    ed.load(img, model.unwrap_or_else(|| EditModel::new(full.0, full.1)));
    let win = gtk::Window::new();
    win.set_default_size(900, 600);
    win.set_child(Some(&ed));
    win.present();
    gtk::test_widget_wait_for_draw(&win);
    (win, ed)
}

/// The canvas as drawn, as `(width, height, BGRA premultiplied bytes)`.
struct Shot {
    w: usize,
    h: usize,
    px: Vec<u8>,
}

impl Shot {
    fn take(widget: &gtk::Widget) -> Shot {
        // The paintable is empty while a redraw is pending.
        gtk::test_widget_wait_for_draw(&widget.root().unwrap());
        let (w, h) = (widget.width(), widget.height());
        let paintable = gtk::WidgetPaintable::new(Some(widget));
        let snap = gtk::Snapshot::new();
        paintable.snapshot(&snap, f64::from(w), f64::from(h));
        let node = snap.to_node().expect("canvas draws something");
        let renderer = widget.native().unwrap().renderer().unwrap();
        let bounds = graphene::Rect::new(0.0, 0.0, w as f32, h as f32);
        let tex = renderer.render_texture(&node, Some(&bounds));
        let (w, h) = (w as usize, h as usize);
        let mut px = vec![0; w * h * 4];
        tex.download(&mut px, w * 4);
        Shot { w, h, px }
    }

    fn rgb(&self, x: usize, y: usize) -> [u8; 3] {
        let i = (y * self.w + x) * 4;
        [self.px[i + 2], self.px[i + 1], self.px[i]]
    }

    /// Opaque black, not the transparent canvas around the image.
    fn black(&self, x: usize, y: usize) -> bool {
        self.px[(y * self.w + x) * 4 + 3] == 255 && self.rgb(x, y).iter().all(|&c| c < 16)
    }
}

/// Simulates a press at `a` released at `b`, as the GestureDrag does.
fn drag(ed: &RedactionEditor, a: (f64, f64), b: (f64, f64)) {
    ed.drag_begin(a.0, a.1);
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    ed.drag_update(dx / 2.0, dy / 2.0);
    ed.drag_update(dx, dy);
    ed.drag_end(dx, dy);
}

fn areas(ed: &RedactionEditor) -> Vec<(AreaKind, Rect)> {
    ed.model()
        .unwrap()
        .areas()
        .iter()
        .map(|a| (a.kind, a.rect))
        .collect()
}

fn all_buttons(w: &gtk::Widget, out: &mut Vec<gtk::Button>) {
    if let Some(b) = w.downcast_ref::<gtk::Button>() {
        out.push(b.clone());
    }
    let mut c = w.first_child();
    while let Some(child) = c {
        all_buttons(&child, out);
        c = child.next_sibling();
    }
}

fn every_control_has_an_accessible_name() {
    let (win, ed) = editor((100, 100), (100, 100), None);
    let mut buttons = Vec::new();
    all_buttons(ed.upcast_ref(), &mut buttons);
    assert!(buttons.len() >= 6, "tools plus undo, redo, delete");
    for b in &buttons {
        assert!(
            gtk::test_accessible_has_property(b, gtk::AccessibleProperty::Label),
            "button {:?} has no accessible label",
            b.label()
        );
        assert!(b.label().is_some_and(|l| !l.is_empty()));
        assert!(b.tooltip_text().is_some());
    }
    let canvas = ed.canvas();
    assert!(canvas.is_focusable());
    assert!(gtk::test_accessible_has_role(
        &canvas,
        gtk::AccessibleRole::Img
    ));
    assert!(gtk::test_accessible_has_property(
        &canvas,
        gtk::AccessibleProperty::Label
    ));
    win.destroy();
}

fn tool_buttons_follow_the_tool() {
    let (win, ed) = editor((100, 100), (100, 100), None);
    assert_eq!(ed.tool(), Tool::Box);
    let mut buttons = Vec::new();
    all_buttons(ed.upcast_ref(), &mut buttons);
    let toggle = |name: &str| {
        buttons
            .iter()
            .find(|b| b.label().as_deref() == Some(name))
            .and_then(|b| b.downcast_ref::<gtk::ToggleButton>().cloned())
            .unwrap()
    };
    assert!(toggle("Box").is_active());
    ed.activate_action("editor.tool", Some(&"pixelate".to_variant()))
        .unwrap();
    assert_eq!(ed.tool(), Tool::Pixelate);
    assert!(toggle("Pixelate").is_active() && !toggle("Box").is_active());
    toggle("Crop").emit_clicked();
    assert_eq!(ed.tool(), Tool::Crop);
    ed.set_tool(Tool::Box);
    assert!(toggle("Box").is_active() && !toggle("Crop").is_active());
    win.destroy();
}

fn drag_draws_a_box_that_is_black_on_screen() {
    let (win, ed) = editor((1000, 500), (200, 100), None);
    let changes = Rc::new(Cell::new(0));
    let c = changes.clone();
    ed.connect_changed(move |_| c.set(c.get() + 1));
    let v = ed.view();
    let (a, b) = (v.to_widget((103.3, 51.7)), v.to_widget((402.6, 210.2)));
    drag(&ed, a, b);
    assert_eq!(areas(&ed), [(AreaKind::Box, v.rect_to_image(a, b))]);
    assert!(changes.get() >= 1);
    assert_eq!(ed.model().unwrap().selected(), Some(0));
    let (crop, redactions) = ed.to_options();
    assert_eq!(crop, None);
    assert_eq!(redactions.len(), 1);
    let shot = Shot::take(&ed.canvas());
    let mid = v.to_widget((250.0, 130.0));
    let out = v.to_widget((450.0, 130.0));
    assert!(shot.black(mid.0 as usize, mid.1 as usize));
    assert!(!shot.black(out.0 as usize, out.1 as usize));

    // One drag is one undo step.
    ed.activate_action("editor.undo", None).unwrap();
    assert!(areas(&ed).is_empty());
    ed.activate_action("editor.redo", None).unwrap();
    assert_eq!(areas(&ed).len(), 1);
    win.destroy();
}

/// The black on screen covers exactly the model rect: never less, and
/// never more, even when the preview is a fifth of the full size and the
/// rect does not fall on preview pixels.
fn box_on_screen_matches_the_model() {
    let mut m = EditModel::new(1000, 500);
    let r = Rect::new(13, 7, 101, 53);
    m.add(AreaKind::Box, r);
    m.select(None);
    let (win, ed) = editor((1000, 500), (200, 100), Some(m));
    let canvas = ed.canvas();
    let shot = Shot::take(&canvas);
    let (x, y, w, h) = ed.view().rect_to_widget(r);
    assert!(
        ed.view().scale > 0.5,
        "preview pixels are several points wide"
    );
    let mid_y = (y + h / 2.0) as usize;
    let mid_x = (x + w / 2.0) as usize;
    let row: Vec<usize> = (0..shot.w).filter(|&i| shot.black(i, mid_y)).collect();
    let col: Vec<usize> = (0..shot.h).filter(|&j| shot.black(mid_x, j)).collect();
    let near = |got: usize, want: f64| (got as f64 - want).abs() <= 1.0;
    assert!(
        near(row[0], x) && near(*row.last().unwrap() + 1, x + w),
        "{row:?} vs {x} {w}"
    );
    assert!(
        near(col[0], y) && near(*col.last().unwrap() + 1, y + h),
        "{col:?} vs {y} {h}"
    );
    assert_eq!(row.len(), row.last().unwrap() - row[0] + 1, "solid row");
    win.destroy();
}

fn click_selects_and_never_adds() {
    let mut m = EditModel::new(200, 100);
    m.add(AreaKind::Box, Rect::new(20, 20, 40, 30));
    m.select(None);
    let (win, ed) = editor((200, 100), (200, 100), Some(m));
    let v = ed.view();
    let changes = Rc::new(Cell::new(0));
    let c = changes.clone();
    ed.connect_changed(move |_| c.set(c.get() + 1));

    // Click inside the area selects it.
    let p = v.to_widget((30.5, 30.5));
    ed.drag_begin(p.0, p.1);
    ed.drag_end(0.0, 0.0);
    assert_eq!(ed.model().unwrap().selected(), Some(0));

    // A click on a pixel boundary and a click inside a pixel, outside any
    // area, both only clear the selection. A small jitter is still a click.
    for (pt, jitter) in [
        ((100.0, 70.0), 0.0),
        ((100.5, 70.5), 0.0),
        ((120.0, 80.0), 2.0),
    ] {
        ed.drag_begin(p.0, p.1);
        ed.drag_end(0.0, 0.0);
        let p = v.to_widget(pt);
        ed.drag_begin(p.0, p.1);
        ed.drag_update(jitter, 0.0);
        ed.drag_end(jitter, jitter);
        let m = ed.model().unwrap();
        assert_eq!(m.selected(), None, "{pt:?}");
        assert_eq!(m.areas().len(), 1, "{pt:?}");
    }
    assert_eq!(changes.get(), 0);

    // A real drag along a pixel boundary (zero height) still draws an
    // area, grown to the minimum size.
    drag(&ed, v.to_widget((100.0, 70.0)), v.to_widget((140.0, 70.0)));
    let got = areas(&ed);
    assert_eq!(got.len(), 2);
    assert!(got[1].1.height >= 4 && got[1].1.width >= 40, "{got:?}");
    assert!(changes.get() >= 1);
    win.destroy();
}

fn drag_moves_and_resizes_the_selected_area() {
    let mut m = EditModel::new(200, 100);
    m.add(AreaKind::Pixelate, Rect::new(20, 20, 40, 30));
    let (win, ed) = editor((200, 100), (200, 100), Some(m));
    let v = ed.view();
    let s = v.scale;

    // Drag from inside: move by 10, 5 image px.
    drag(&ed, v.to_widget((30.0, 30.0)), v.to_widget((40.0, 35.0)));
    assert_eq!(
        areas(&ed),
        [(AreaKind::Pixelate, Rect::new(30, 25, 40, 30))]
    );

    // Drag the bottom-right handle out; the end rounds outward.
    drag(&ed, v.to_widget((70.0, 55.0)), v.to_widget((80.4, 65.4)));
    assert_eq!(areas(&ed)[0].1, Rect::new(30, 25, 51, 41));

    // Drag the top-left handle past the image corner: clamped to it.
    let tl = v.to_widget((30.0, 25.0));
    drag(&ed, tl, (tl.0 - 100.0 * s, tl.1 - 100.0 * s));
    assert_eq!(areas(&ed)[0].1, Rect::new(0, 0, 81, 66));

    // Moving past the edge stops at the edge, keeping the size.
    drag(&ed, v.to_widget((40.0, 40.0)), v.to_widget((400.0, 40.0)));
    assert_eq!(areas(&ed)[0].1, Rect::new(119, 0, 81, 66));

    for want in [
        Rect::new(0, 0, 81, 66),
        Rect::new(30, 25, 51, 41),
        Rect::new(30, 25, 40, 30),
        Rect::new(20, 20, 40, 30),
    ] {
        ed.activate_action("editor.undo", None).unwrap();
        assert_eq!(areas(&ed)[0].1, want);
    }
    win.destroy();
}

fn crop_rounds_inward_and_dims_outside() {
    let (win, ed) = editor((200, 100), (200, 100), None);
    ed.set_tool(Tool::Crop);
    let v = ed.view();
    let (a, b) = (v.to_widget((50.4, 20.6)), v.to_widget((150.5, 80.2)));
    drag(&ed, a, b);
    let m = ed.model().unwrap();
    assert!(m.areas().is_empty());
    assert_eq!(m.crop(), Some(Rect::new(51, 21, 99, 59)));
    assert_eq!(m.crop(), v.crop_to_image(a, b));

    let shot = Shot::take(&ed.canvas());
    let inside = v.to_widget((100.0, 50.0));
    let outside = v.to_widget((20.0, 50.0));
    let lum = |p: (f64, f64)| shot.rgb(p.0 as usize, p.1 as usize)[1];
    assert_eq!(lum(inside), GRAY);
    assert!(lum(outside) < GRAY / 2, "outside the crop is dimmed");

    // Delete with the Crop tool clears the crop.
    ed.activate_action("editor.delete", None).unwrap();
    assert_eq!(ed.model().unwrap().crop(), None);
    win.destroy();
}

fn actions_are_enabled_only_when_they_apply() {
    let (win, ed) = editor((200, 100), (200, 100), None);
    let enabled = |a: &str| {
        // The toolbar button for the action is sensitive iff it applies.
        let mut buttons = Vec::new();
        all_buttons(ed.upcast_ref(), &mut buttons);
        buttons
            .iter()
            .find(|b| b.action_name().as_deref() == Some(a))
            .unwrap()
            .is_sensitive()
    };
    assert!(!enabled("editor.undo") && !enabled("editor.redo") && !enabled("editor.delete"));
    let v = ed.view();
    drag(&ed, v.to_widget((10.0, 10.0)), v.to_widget((60.0, 60.0)));
    assert!(enabled("editor.undo") && enabled("editor.delete"));
    ed.activate_action("editor.delete", None).unwrap();
    assert!(areas(&ed).is_empty());
    assert!(!enabled("editor.delete"));
    ed.activate_action("editor.undo", None).unwrap();
    assert_eq!(areas(&ed).len(), 1);
    assert!(enabled("editor.redo"));
    // Undo clears the selection, so there is nothing to delete.
    assert!(!enabled("editor.delete"));
    win.destroy();
}

fn pixelate_preview_changes_pixels_inside_only() {
    let mut m = EditModel::new(200, 100);
    m.add(AreaKind::Pixelate, Rect::new(40, 20, 80, 60));
    m.select(None);
    let ed = RedactionEditor::new();
    // A gradient, so averaging blocks changes it.
    let img = RgbaImage::from_fn(200, 100, |x, _| Rgba([x as u8, 100, 100, 255]));
    ed.load(img, m);
    let win = gtk::Window::new();
    win.set_default_size(900, 600);
    win.set_child(Some(&ed));
    win.present();
    gtk::test_widget_wait_for_draw(&win);
    let v = ed.view();
    let shot = Shot::take(&ed.canvas());
    let px = |x: f64, y: f64| {
        let p = v.to_widget((x + 0.5, y + 0.5));
        shot.rgb(p.0 as usize, p.1 as usize)
    };
    // Outside: the gradient. Inside: one block of 16 px has one color.
    assert!(px(10.0, 10.0)[0].abs_diff(10) <= 1);
    assert!(px(150.0, 50.0)[0].abs_diff(150) <= 1);
    assert_eq!(px(41.0, 21.0), px(54.0, 34.0));
    win.destroy();
}

fn shortcuts_map_each_trigger_to_its_action() {
    let (win, ed) = editor((100, 100), (100, 100), None);
    let controllers = ed.observe_controllers();
    let mut found = Vec::new();
    for i in 0..controllers.n_items() {
        let Some(sc) = controllers
            .item(i)
            .and_downcast::<gtk::ShortcutController>()
        else {
            continue;
        };
        for j in 0..sc.n_items() {
            let s = sc.item(j).and_downcast::<gtk::Shortcut>().unwrap();
            let trigger = s.trigger().unwrap().to_str().to_string();
            let action = s
                .action()
                .and_downcast::<gtk::NamedAction>()
                .unwrap()
                .action_name()
                .to_string();
            let arg = s.arguments().and_then(|v| v.get::<String>());
            found.push((trigger, action, arg));
        }
    }
    let want = [
        ("b", "editor.tool", Some("box")),
        ("p", "editor.tool", Some("pixelate")),
        ("c", "editor.tool", Some("crop")),
        ("<Control>z", "editor.undo", None),
        ("<Control><Shift>z", "editor.redo", None),
        ("<Control>y", "editor.redo", None),
        ("Delete", "editor.delete", None),
        ("BackSpace", "editor.delete", None),
    ];
    for (t, a, arg) in want {
        let t = gtk::ShortcutTrigger::parse_string(t)
            .unwrap()
            .to_str()
            .to_string();
        let w = (t, a.to_string(), arg.map(str::to_string));
        assert!(found.contains(&w), "{w:?} not in {found:?}");
    }
    // Each target action exists and does what it says.
    ed.activate_action("editor.tool", Some(&"crop".to_variant()))
        .unwrap();
    assert_eq!(ed.tool(), Tool::Crop);
    win.destroy();
}

fn keyboard_adds_moves_resizes_and_deletes() {
    use gdk::Key;
    use gdk::ModifierType as M;
    let (win, ed) = editor((200, 100), (200, 100), None);
    ed.canvas().grab_focus();
    let step = (10.0 / ed.view().scale).round().max(1.0) as u32;
    let rect = |ed: &RedactionEditor| areas(ed)[0].1;

    assert!(ed.handle_key(Key::Return, M::empty()));
    assert_eq!(areas(&ed), [(AreaKind::Box, Rect::new(50, 25, 100, 50))]);
    assert_eq!(ed.model().unwrap().selected(), Some(0));

    // A held arrow (several presses, one release) is one undo step.
    for _ in 0..3 {
        assert!(ed.handle_key(Key::Right, M::empty()));
    }
    ed.handle_key_release(Key::Right);
    assert_eq!(rect(&ed), Rect::new(50 + 3 * step, 25, 100, 50));
    assert!(ed.handle_key(Key::Up, M::ALT_MASK));
    ed.handle_key_release(Key::Up);
    assert_eq!(rect(&ed), Rect::new(50 + 3 * step, 24, 100, 50));
    assert!(ed.handle_key(Key::Down, M::SHIFT_MASK));
    ed.handle_key_release(Key::Down);
    assert_eq!(rect(&ed), Rect::new(50 + 3 * step, 24, 100, 50 + step));
    ed.activate_action("editor.undo", None).unwrap();
    ed.activate_action("editor.undo", None).unwrap();
    ed.activate_action("editor.undo", None).unwrap();
    assert_eq!(areas(&ed)[0].1, Rect::new(50, 25, 100, 50));
    ed.activate_action("editor.redo", None).unwrap();

    // Select again with Tab, then Delete (the shortcut's action).
    assert!(ed.handle_key(Key::Tab, M::empty()));
    ed.activate_action("editor.delete", None).unwrap();
    assert!(areas(&ed).is_empty());
    ed.activate_action("editor.undo", None).unwrap();
    assert_eq!(areas(&ed).len(), 1);

    // Pixelate tool: Enter adds a pixelate area.
    ed.activate_action("editor.tool", Some(&"pixelate".to_variant()))
        .unwrap();
    assert!(ed.handle_key(Key::KP_Enter, M::empty()));
    assert_eq!(areas(&ed)[1].0, AreaKind::Pixelate);

    // Crop: the same keys edit the crop.
    ed.set_tool(Tool::Crop);
    assert!(ed.handle_key(Key::Return, M::empty()));
    assert_eq!(ed.model().unwrap().crop(), Some(Rect::new(50, 25, 100, 50)));
    assert!(ed.handle_key(Key::Left, M::ALT_MASK));
    assert!(ed.handle_key(Key::Left, M::SHIFT_MASK | M::ALT_MASK));
    ed.handle_key_release(Key::Left);
    assert_eq!(ed.model().unwrap().crop(), Some(Rect::new(49, 25, 99, 50)));
    win.destroy();
}

fn tab_cycles_and_escape_leaves_the_canvas() {
    use gdk::Key;
    use gdk::ModifierType as M;
    let mut m = EditModel::new(200, 100);
    m.add(AreaKind::Box, Rect::new(10, 10, 20, 20));
    m.add(AreaKind::Box, Rect::new(50, 10, 20, 20));
    let (win, ed) = editor((200, 100), (200, 100), Some(m));
    let canvas = ed.canvas();
    canvas.grab_focus();
    assert_eq!(
        gtk::prelude::GtkWindowExt::focus(&win).as_ref(),
        Some(&canvas)
    );
    let sel = |ed: &RedactionEditor| ed.model().unwrap().selected();
    assert_eq!(sel(&ed), Some(1));
    assert!(ed.handle_key(Key::Tab, M::empty()));
    assert_eq!(sel(&ed), Some(0));
    assert!(ed.handle_key(Key::ISO_Left_Tab, M::SHIFT_MASK));
    assert_eq!(sel(&ed), Some(1));
    assert!(ed.handle_key(Key::Escape, M::empty()));
    assert_eq!(sel(&ed), None);
    assert_eq!(
        gtk::prelude::GtkWindowExt::focus(&win).as_ref(),
        Some(&canvas)
    );
    assert!(ed.handle_key(Key::Escape, M::empty()));
    let focus = gtk::prelude::GtkWindowExt::focus(&win).unwrap();
    assert!(
        focus.is_ancestor(&ed.toolbar()),
        "focus left for the toolbar"
    );

    // With no areas, Tab is not used, so focus can move on.
    ed.activate_action("editor.next-area", None).unwrap();
    ed.activate_action("editor.delete", None).unwrap();
    ed.activate_action("editor.next-area", None).unwrap();
    ed.activate_action("editor.delete", None).unwrap();
    assert!(areas(&ed).is_empty());
    assert!(!ed.handle_key(Key::Tab, M::empty()));
    assert!(!ed.handle_key(Key::Right, M::empty()));
    win.destroy();
}

fn has_description(w: &gtk::Widget, text: &str) -> bool {
    use gtk::glib::translate::ToGlibPtr;
    let c = std::ffi::CString::new(text).unwrap();
    let a: &gtk::Accessible = w.upcast_ref();
    // SAFETY: a live accessible and a NUL-terminated string, as the
    // description property expects; the result is freed with g_free.
    unsafe {
        let r = gtk::ffi::gtk_test_accessible_check_property(
            a.to_glib_none().0,
            gtk::ffi::GTK_ACCESSIBLE_PROPERTY_DESCRIPTION,
            c.as_ptr(),
        );
        if r.is_null() {
            true
        } else {
            gtk::glib::ffi::g_free(r.cast());
            false
        }
    }
}

fn description_follows_the_edits() {
    let (win, ed) = editor((1000, 800), (250, 200), None);
    let canvas = ed.canvas();
    assert_eq!(ed.description(), "No areas.");
    assert!(has_description(&canvas, "No areas."));
    assert!(!has_description(&canvas, "1 area: 1 box."));
    ed.activate_action("editor.new-area", None).unwrap();
    let want = "1 area: 1 box. Selected: box 1, 500 by 400 pixels at 250, 200.";
    assert_eq!(ed.description(), want);
    assert!(has_description(&canvas, want));
    ed.activate_action("editor.move-by", Some(&(10, -20).to_variant()))
        .unwrap();
    let want = "1 area: 1 box. Selected: box 1, 500 by 400 pixels at 260, 180.";
    assert!(has_description(&canvas, want), "{}", ed.description());
    ed.activate_action("editor.commit", None).unwrap();
    assert!(has_description(&canvas, "1 area: 1 box."));
    win.destroy();
}

fn warning_shows_only_for_pixelate() {
    let (win, ed) = editor((200, 100), (200, 100), None);
    assert!(!ed.warning_visible());
    ed.set_tool(Tool::Pixelate);
    assert!(ed.warning_visible());
    ed.activate_action("editor.new-area", None).unwrap();
    ed.set_tool(Tool::Box);
    // A selected pixelate area keeps the warning up.
    assert!(ed.warning_visible());
    ed.activate_action("editor.commit", None).unwrap();
    assert!(!ed.warning_visible());
    ed.activate_action("editor.new-area", None).unwrap();
    assert!(!ed.warning_visible());
    ed.set_tool(Tool::Crop);
    assert!(!ed.warning_visible());
    win.destroy();
}

fn main() -> ExitCode {
    let env = headless::Headless::start();
    env.watchdog(WATCHDOG);
    gtk::init().expect("GTK init under headless weston");
    let _ = gdk::Display::default().expect("a display");

    let code = headless::run(&[
        (
            "every_control_has_an_accessible_name",
            every_control_has_an_accessible_name,
        ),
        ("tool_buttons_follow_the_tool", tool_buttons_follow_the_tool),
        (
            "drag_draws_a_box_that_is_black_on_screen",
            drag_draws_a_box_that_is_black_on_screen,
        ),
        (
            "box_on_screen_matches_the_model",
            box_on_screen_matches_the_model,
        ),
        ("click_selects_and_never_adds", click_selects_and_never_adds),
        (
            "drag_moves_and_resizes_the_selected_area",
            drag_moves_and_resizes_the_selected_area,
        ),
        (
            "crop_rounds_inward_and_dims_outside",
            crop_rounds_inward_and_dims_outside,
        ),
        (
            "actions_are_enabled_only_when_they_apply",
            actions_are_enabled_only_when_they_apply,
        ),
        (
            "pixelate_preview_changes_pixels_inside_only",
            pixelate_preview_changes_pixels_inside_only,
        ),
        (
            "shortcuts_map_each_trigger_to_its_action",
            shortcuts_map_each_trigger_to_its_action,
        ),
        (
            "keyboard_adds_moves_resizes_and_deletes",
            keyboard_adds_moves_resizes_and_deletes,
        ),
        (
            "tab_cycles_and_escape_leaves_the_canvas",
            tab_cycles_and_escape_leaves_the_canvas,
        ),
        (
            "description_follows_the_edits",
            description_follows_the_edits,
        ),
        (
            "warning_shows_only_for_pixelate",
            warning_shows_only_for_pixelate,
        ),
    ]);
    drop(env);
    code
}
