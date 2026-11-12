//! The redaction editor widget (M2 plan, "Widget"): a toolbar and a canvas
//! that shows the preview with the edits burned in by core.

use std::cell::{Cell, RefCell};
use std::sync::OnceLock;

use gtk::glib::subclass::Signal;
use gtk::prelude::*;
use gtk::subclass::prelude::*;
use gtk::{gdk, glib, graphene, gsk};
use image::RgbaImage;
use safe_upload_core::edit::{Rect, Redaction};

use crate::canvas::Canvas;
use crate::model::{Area, EditModel, Tool};
use crate::preview;
use crate::view::ViewTransform;

/// Side of a handle square, in widget points.
const HANDLE: f64 = 10.0;
/// A press that moves less than this (widget points) is a click, not a
/// drag. A click selects; it never draws.
pub const CLICK_SLOP: f64 = 4.0;

pub(crate) fn tool_name(t: Tool) -> &'static str {
    match t {
        Tool::Box => "box",
        Tool::Pixelate => "pixelate",
        Tool::Crop => "crop",
    }
}

fn tool_from_name(s: &str) -> Option<Tool> {
    match s {
        "box" => Some(Tool::Box),
        "pixelate" => Some(Tool::Pixelate),
        "crop" => Some(Tool::Crop),
        _ => None,
    }
}

/// What the arrow keys, handles and Delete act on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Target {
    Area(usize),
    Crop,
}

#[derive(Debug, Clone, Copy)]
enum DragKind {
    Draw,
    Move(Target, Rect),
    /// Resize from a corner; the opposite corner stays at this image point.
    Resize(Target, (u32, u32)),
}

#[derive(Debug, Clone, Copy)]
struct Drag {
    start: (f64, f64),
    kind: DragKind,
    active: bool,
    drawn: Option<usize>,
}

mod imp {
    use super::*;

    #[derive(Default)]
    pub struct RedactionEditor {
        pub(super) model: RefCell<Option<EditModel>>,
        pub(super) tool: Cell<Tool>,
        pub(super) base: RefCell<Option<gdk::Texture>>,
        pub(super) preview: RefCell<Option<RgbaImage>>,
        /// `None` with a loaded preview means core could not redact it;
        /// areas are then drawn solid black.
        pub(super) edited: RefCell<Option<gdk::Texture>>,
        pub(super) drag: Cell<Option<Drag>>,
        pub(super) toolbar: RefCell<Option<gtk::Box>>,
        pub(super) canvas: RefCell<Option<Canvas>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for RedactionEditor {
        const NAME: &'static str = "SafeUploadRedactionEditor";
        type Type = super::RedactionEditor;
        type ParentType = gtk::Widget;

        fn class_init(klass: &mut Self::Class) {
            klass.set_layout_manager_type::<gtk::BoxLayout>();
            klass.set_css_name("redaction-editor");
            klass.install_property_action("editor.tool", "tool");
            klass.install_action("editor.undo", None, |ed, _, _| ed.undo());
            klass.install_action("editor.redo", None, |ed, _, _| ed.redo());
            klass.install_action("editor.delete", None, |ed, _, _| ed.delete_target());
        }
    }

    impl ObjectImpl for RedactionEditor {
        fn properties() -> &'static [glib::ParamSpec] {
            static P: OnceLock<Vec<glib::ParamSpec>> = OnceLock::new();
            P.get_or_init(|| {
                vec![
                    glib::ParamSpecString::builder("tool")
                        .default_value(Some("box"))
                        .explicit_notify()
                        .build(),
                ]
            })
        }

        fn set_property(&self, _id: usize, value: &glib::Value, pspec: &glib::ParamSpec) {
            if pspec.name() == "tool"
                && let Some(t) = value.get::<Option<String>>().ok().flatten()
                && let Some(t) = tool_from_name(&t)
            {
                self.obj().set_tool(t);
            }
        }

        fn property(&self, _id: usize, pspec: &glib::ParamSpec) -> glib::Value {
            match pspec.name() {
                "tool" => tool_name(self.tool.get()).to_value(),
                _ => unreachable!(),
            }
        }

        fn signals() -> &'static [Signal] {
            static S: OnceLock<Vec<Signal>> = OnceLock::new();
            S.get_or_init(|| vec![Signal::builder("changed").build()])
        }

        fn constructed(&self) {
            self.parent_constructed();
            let obj = self.obj();
            if let Some(layout) = obj.layout_manager().and_downcast::<gtk::BoxLayout>() {
                layout.set_orientation(gtk::Orientation::Vertical);
                layout.set_spacing(6);
            }
            let toolbar = super::build_toolbar();
            toolbar.set_parent(&*obj);
            let canvas = Canvas::new();
            canvas.set_parent(&*obj);

            let drag = gtk::GestureDrag::new();
            drag.connect_drag_begin(glib::clone!(
                #[weak]
                obj,
                move |_, x, y| obj.drag_begin(x, y)
            ));
            drag.connect_drag_update(glib::clone!(
                #[weak]
                obj,
                move |_, dx, dy| obj.drag_update(dx, dy)
            ));
            drag.connect_drag_end(glib::clone!(
                #[weak]
                obj,
                move |_, dx, dy| obj.drag_end(dx, dy)
            ));
            drag.connect_cancel(glib::clone!(
                #[weak]
                obj,
                move |_, _| obj.drag_cancel()
            ));
            canvas.add_controller(drag);

            self.toolbar.replace(Some(toolbar));
            self.canvas.replace(Some(canvas));
            obj.update_state();
        }

        fn dispose(&self) {
            while let Some(child) = self.obj().first_child() {
                child.unparent();
            }
        }
    }

    impl WidgetImpl for RedactionEditor {}
}

glib::wrapper! {
    /// The redaction editor. Load a preview and an [`EditModel`] with
    /// [`load`](Self::load), listen to `changed`, read
    /// [`to_options`](Self::to_options).
    pub struct RedactionEditor(ObjectSubclass<imp::RedactionEditor>)
        @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl Default for RedactionEditor {
    fn default() -> Self {
        Self::new()
    }
}

fn tool_button(label: &str, tool: Tool, tooltip: &str) -> gtk::ToggleButton {
    let b = gtk::ToggleButton::with_label(label);
    b.set_action_name(Some("editor.tool"));
    b.set_action_target(Some(tool_name(tool)));
    b.set_tooltip_text(Some(tooltip));
    b.update_property(&[gtk::accessible::Property::Label(label)]);
    b
}

fn action_button(label: &str, action: &str, tooltip: &str) -> gtk::Button {
    let b = gtk::Button::with_label(label);
    b.set_action_name(Some(action));
    b.set_tooltip_text(Some(tooltip));
    b.update_property(&[gtk::accessible::Property::Label(label)]);
    b
}

fn build_toolbar() -> gtk::Box {
    let tools = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    tools.add_css_class("linked");
    tools.append(&tool_button(
        "Box",
        Tool::Box,
        "Cover with a solid black box. Safe for text.",
    ));
    tools.append(&tool_button(
        "Pixelate",
        Tool::Pixelate,
        "Pixelate an area. Not safe for text.",
    ));
    tools.append(&tool_button("Crop", Tool::Crop, "Crop the image"));

    let bar = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    bar.append(&tools);
    bar.append(&action_button("Undo", "editor.undo", "Undo the last edit"));
    bar.append(&action_button(
        "Redo",
        "editor.redo",
        "Redo the last undone edit",
    ));
    bar.append(&action_button(
        "Delete",
        "editor.delete",
        "Delete the selected area, or the crop with the Crop tool",
    ));
    bar
}

fn to_graphene((x, y, w, h): (f64, f64, f64, f64)) -> graphene::Rect {
    graphene::Rect::new(x as f32, y as f32, w as f32, h as f32)
}

fn contains(r: Rect, (x, y): (f64, f64)) -> bool {
    x >= f64::from(r.x)
        && y >= f64::from(r.y)
        && x <= f64::from(r.x) + f64::from(r.width)
        && y <= f64::from(r.y) + f64::from(r.height)
}

/// Whole pixels from `anchor` to the pointer at `p`, rounded outward, at
/// least one pixel.
fn outward_span(anchor: u32, p: f64) -> (u32, u32) {
    let p = p.max(0.0);
    if p < f64::from(anchor) {
        (p.floor() as u32, anchor)
    } else {
        (anchor, (p.ceil() as u32).max(anchor.saturating_add(1)))
    }
}

/// Whole pixels from `anchor` to the pointer at `p`, rounded inward.
fn inward_span(anchor: u32, p: f64) -> (u32, u32) {
    let p = p.max(0.0);
    if p < f64::from(anchor) {
        ((p.ceil() as u32).min(anchor), anchor)
    } else {
        (anchor, (p.floor() as u32).max(anchor))
    }
}

/// The rect drawn from widget point `a` to `b`, rounded outward and at
/// least one pixel per side. `None` if it misses the image.
fn drawn_rect(
    view: &ViewTransform,
    size: (u32, u32),
    a: (f64, f64),
    b: (f64, f64),
) -> Option<Rect> {
    let (ia, ib) = (view.to_image(a), view.to_image(b));
    let misses = |p: f64, q: f64, limit: u32| p.max(q) <= 0.0 || p.min(q) >= f64::from(limit);
    if misses(ia.0, ib.0, size.0) || misses(ia.1, ib.1, size.1) {
        return None;
    }
    let mut r = view.rect_to_image(a, b);
    r.width = r.width.max(1);
    r.height = r.height.max(1);
    Some(r)
}

fn snapshot_of(m: &EditModel) -> (Vec<Area>, Option<Rect>) {
    (m.areas().to_vec(), m.crop())
}

impl RedactionEditor {
    pub fn new() -> Self {
        glib::Object::new()
    }

    /// Shows `preview` (the downscaled, oriented sRGB pixels) and edits
    /// `model`, whose size is the full oriented size. Selection and undo
    /// history come with the model.
    pub fn load(&self, preview: RgbaImage, model: EditModel) {
        let imp = self.imp();
        let (w, h) = preview.dimensions();
        let bytes = glib::Bytes::from(preview.as_raw().as_slice());
        let texture = gdk::MemoryTexture::new(
            w as i32,
            h as i32,
            gdk::MemoryFormat::R8g8b8a8,
            &bytes,
            w as usize * 4,
        );
        imp.drag.set(None);
        imp.base.replace(Some(texture.upcast()));
        imp.preview.replace(Some(preview));
        imp.model.replace(Some(model));
        self.refresh_preview();
        self.update_state();
    }

    /// A copy of the edit model.
    pub fn model(&self) -> Option<EditModel> {
        self.imp().model.borrow().clone()
    }

    /// The crop and redactions for `core::Options` (see
    /// [`EditModel::to_options`]).
    pub fn to_options(&self) -> (Option<Rect>, Vec<Redaction>) {
        self.imp()
            .model
            .borrow()
            .as_ref()
            .map(EditModel::to_options)
            .unwrap_or_default()
    }

    pub fn tool(&self) -> Tool {
        self.imp().tool.get()
    }

    pub fn set_tool(&self, tool: Tool) {
        if self.imp().tool.replace(tool) != tool {
            self.notify("tool");
            self.update_state();
        }
    }

    /// The canvas the image is drawn on (focusable, role `Img`).
    pub fn canvas(&self) -> gtk::Widget {
        self.imp().canvas.borrow().clone().expect("built").upcast()
    }

    /// The toolbar.
    pub fn toolbar(&self) -> gtk::Widget {
        self.imp().toolbar.borrow().clone().expect("built").upcast()
    }

    /// Maps canvas points to full-size image pixels.
    pub fn view(&self) -> ViewTransform {
        let canvas = self.canvas();
        let size = self.size_px();
        ViewTransform::fit(
            size,
            (f64::from(canvas.width()), f64::from(canvas.height())),
        )
    }

    pub fn connect_changed<F: Fn(&Self) + 'static>(&self, f: F) -> glib::SignalHandlerId {
        self.connect_local("changed", false, move |args| {
            f(&args[0].get::<Self>().expect("editor"));
            None
        })
    }

    /// Starts a press at canvas point `(x, y)`. The canvas `GestureDrag`
    /// calls this; it is public so tests can drive it.
    pub fn drag_begin(&self, x: f64, y: f64) {
        self.canvas().grab_focus();
        let Some(kind) = self.with_model(|m| self.drag_kind(m, (x, y))) else {
            return;
        };
        self.imp().drag.set(Some(Drag {
            start: (x, y),
            kind,
            active: false,
            drawn: None,
        }));
    }

    /// The pointer moved by `(dx, dy)` from the press.
    pub fn drag_update(&self, dx: f64, dy: f64) {
        let Some(mut d) = self.imp().drag.get() else {
            return;
        };
        if !d.active {
            if dx.hypot(dy) < CLICK_SLOP {
                return;
            }
            d.active = true;
            self.edit(|m| m.begin_gesture());
        }
        let view = self.view();
        let size = self.size_px();
        let p = (d.start.0 + dx, d.start.1 + dy);
        let tool = self.tool();
        self.edit(|m| match d.kind {
            DragKind::Draw => match tool.area_kind() {
                Some(kind) => match (d.drawn, drawn_rect(&view, size, d.start, p)) {
                    (None, Some(r)) => d.drawn = m.add(kind, r),
                    (Some(i), Some(r)) => m.set_rect(i, r),
                    _ => {}
                },
                None => {
                    if let Some(c) = view.crop_to_image(d.start, p) {
                        m.set_crop(Some(c));
                    }
                }
            },
            DragKind::Move(target, orig) => {
                let ddx = (dx / view.scale).round() as i64;
                let ddy = (dy / view.scale).round() as i64;
                let Some(cur) = target_rect(m, target) else {
                    return;
                };
                let step = |o: u32, c: u32, d: i64| {
                    (i64::from(o) + d - i64::from(c)).clamp(i32::MIN.into(), i32::MAX.into()) as i32
                };
                let (sx, sy) = (step(orig.x, cur.x, ddx), step(orig.y, cur.y, ddy));
                match target {
                    Target::Area(i) => m.move_by(i, sx, sy),
                    Target::Crop => m.move_crop_by(sx, sy),
                }
            }
            DragKind::Resize(target, anchor) => {
                let ip = view.to_image(p);
                match target {
                    Target::Area(i) => {
                        let (x0, x1) = outward_span(anchor.0, ip.0);
                        let (y0, y1) = outward_span(anchor.1, ip.1);
                        m.set_rect(i, Rect::new(x0, y0, x1 - x0, y1 - y0));
                    }
                    Target::Crop => {
                        let (x0, x1) = inward_span(anchor.0, ip.0);
                        let (y0, y1) = inward_span(anchor.1, ip.1);
                        m.set_crop(Some(Rect::new(x0, y0, x1 - x0, y1 - y0)));
                    }
                }
            }
        });
        self.imp().drag.set(Some(d));
    }

    /// The press ended after moving `(dx, dy)`. Less than [`CLICK_SLOP`]
    /// is a click: it selects the topmost area under the pointer, or
    /// clears the selection. A click never adds an area.
    pub fn drag_end(&self, dx: f64, dy: f64) {
        self.drag_update(dx, dy);
        let Some(d) = self.imp().drag.take() else {
            return;
        };
        if d.active {
            self.edit(|m| m.end_gesture());
            return;
        }
        let p = self.view().to_image(d.start);
        self.edit(|m| {
            let hit = m.areas().iter().rposition(|a| contains(a.rect, p));
            m.select(hit);
        });
    }

    fn drag_cancel(&self) {
        if self.imp().drag.take().is_some() {
            self.edit(|m| m.end_gesture());
        }
    }

    fn drag_kind(&self, m: &EditModel, p: (f64, f64)) -> DragKind {
        let view = self.view();
        if let Some(t) = self.target(m)
            && let Some(r) = target_rect(m, t)
        {
            let (x, y, w, h) = view.rect_to_widget(r);
            let corners = [
                ((x, y), (r.x + r.width, r.y + r.height)),
                ((x + w, y), (r.x, r.y + r.height)),
                ((x, y + h), (r.x + r.width, r.y)),
                ((x + w, y + h), (r.x, r.y)),
            ];
            for (c, anchor) in corners {
                if (p.0 - c.0).abs() <= HANDLE / 2.0 && (p.1 - c.1).abs() <= HANDLE / 2.0 {
                    return DragKind::Resize(t, anchor);
                }
            }
            if contains(r, view.to_image(p)) {
                return DragKind::Move(t, r);
            }
        }
        DragKind::Draw
    }

    /// The selected area, or the crop when the Crop tool is active.
    pub(crate) fn target(&self, m: &EditModel) -> Option<Target> {
        match self.tool() {
            Tool::Crop => m.crop().map(|_| Target::Crop),
            _ => m.selected().map(Target::Area),
        }
    }

    fn undo(&self) {
        self.edit(|m| {
            m.undo();
        });
    }

    fn redo(&self) {
        self.edit(|m| {
            m.redo();
        });
    }

    fn delete_target(&self) {
        let tool = self.tool();
        self.edit(|m| match tool {
            Tool::Crop => m.set_crop(None),
            _ => {
                if let Some(i) = m.selected() {
                    m.delete(i);
                }
            }
        });
    }

    fn size_px(&self) -> (u32, u32) {
        self.imp()
            .model
            .borrow()
            .as_ref()
            .map_or((1, 1), EditModel::size)
    }

    fn with_model<R>(&self, f: impl FnOnce(&EditModel) -> R) -> Option<R> {
        self.imp().model.borrow().as_ref().map(f)
    }

    /// Runs `f` on the model. If the areas or crop changed, re-renders the
    /// preview and emits `changed`. Always redraws.
    pub(crate) fn edit(&self, f: impl FnOnce(&mut EditModel)) {
        let changed = {
            let mut model = self.imp().model.borrow_mut();
            let Some(m) = model.as_mut() else {
                return;
            };
            let before = snapshot_of(m);
            f(m);
            before != snapshot_of(m)
        };
        if changed {
            self.refresh_preview();
        }
        self.update_state();
        if changed {
            self.emit_by_name::<()>("changed", &[]);
        }
    }

    fn refresh_preview(&self) {
        let imp = self.imp();
        let edited = {
            let preview = imp.preview.borrow();
            let model = imp.model.borrow();
            match (preview.as_ref(), model.as_ref()) {
                (Some(p), Some(m)) => preview::render(p, m).ok().map(|img| {
                    let (w, h) = img.dimensions();
                    gdk::MemoryTexture::new(
                        w as i32,
                        h as i32,
                        gdk::MemoryFormat::R8g8b8a8,
                        &glib::Bytes::from_owned(img.into_raw()),
                        w as usize * 4,
                    )
                    .upcast::<gdk::Texture>()
                }),
                _ => None,
            }
        };
        imp.edited.replace(edited);
    }

    /// Updates action states and redraws.
    fn update_state(&self) {
        let (can_undo, can_redo, can_delete) = self
            .with_model(|m| (m.can_undo(), m.can_redo(), self.target(m).is_some()))
            .unwrap_or_default();
        self.action_set_enabled("editor.undo", can_undo);
        self.action_set_enabled("editor.redo", can_redo);
        self.action_set_enabled("editor.delete", can_delete);
        if let Some(c) = self.imp().canvas.borrow().as_ref() {
            c.queue_draw();
        }
    }

    pub(crate) fn draw_canvas(&self, snapshot: &gtk::Snapshot) {
        let imp = self.imp();
        let model = imp.model.borrow();
        let (Some(m), Some(base)) = (model.as_ref(), imp.base.borrow().clone()) else {
            return;
        };
        let view = self.view();
        let (fw, fh) = m.size();
        let image = to_graphene(view.rect_to_widget(Rect::new(0, 0, fw, fh)));
        snapshot.append_scaled_texture(&base, gsk::ScalingFilter::Linear, &image);

        // Each area shows the core-redacted preview, clipped to exactly
        // the model rect: never smaller or larger on screen than in the
        // model. Nearest filtering keeps unredacted neighbours out.
        let edited = imp.edited.borrow();
        for a in m.areas() {
            let r = to_graphene(view.rect_to_widget(a.rect));
            match edited.as_ref() {
                Some(t) => {
                    snapshot.push_clip(&r);
                    snapshot.append_scaled_texture(t, gsk::ScalingFilter::Nearest, &image);
                    snapshot.pop();
                }
                None => snapshot.append_color(&gdk::RGBA::BLACK, &r),
            }
        }

        if let Some(c) = m.crop() {
            let (cx, cy, cw, ch) = view.rect_to_widget(c);
            let (ix, iy, iw, ih) = view.rect_to_widget(Rect::new(0, 0, fw, fh));
            let dim = gdk::RGBA::new(0.0, 0.0, 0.0, 0.6);
            for r in [
                (ix, iy, iw, cy - iy),
                (ix, cy + ch, iw, iy + ih - cy - ch),
                (ix, cy, cx - ix, ch),
                (cx + cw, cy, ix + iw - cx - cw, ch),
            ] {
                if r.2 > 0.0 && r.3 > 0.0 {
                    snapshot.append_color(&dim, &to_graphene(r));
                }
            }
            outline(snapshot, (cx, cy, cw, ch), 1.0, gdk::RGBA::WHITE);
        }

        let accent = gdk::RGBA::new(0.21, 0.52, 0.89, 1.0);
        let target = self.target(m);
        for (i, a) in m.areas().iter().enumerate() {
            let selected = target == Some(Target::Area(i));
            let (width, color) = if selected {
                (2.0, accent)
            } else {
                (1.0, gdk::RGBA::new(1.0, 1.0, 1.0, 0.8))
            };
            outline(snapshot, view.rect_to_widget(a.rect), width, color);
        }
        if let Some(r) = target.and_then(|t| target_rect(m, t)) {
            let (x, y, w, h) = view.rect_to_widget(r);
            for (hx, hy) in [(x, y), (x + w, y), (x, y + h), (x + w, y + h)] {
                let s = HANDLE;
                let outer = (hx - s / 2.0, hy - s / 2.0, s, s);
                snapshot.append_color(&gdk::RGBA::WHITE, &to_graphene(outer));
                let inner = (outer.0 + 1.0, outer.1 + 1.0, s - 2.0, s - 2.0);
                snapshot.append_color(&accent, &to_graphene(inner));
            }
        }
    }
}

pub(crate) fn target_rect(m: &EditModel, t: Target) -> Option<Rect> {
    match t {
        Target::Area(i) => m.areas().get(i).map(|a| a.rect),
        Target::Crop => m.crop(),
    }
}

/// A border just outside `(x, y, w, h)`, so it never hides the area.
fn outline(
    snapshot: &gtk::Snapshot,
    (x, y, w, h): (f64, f64, f64, f64),
    width: f64,
    color: gdk::RGBA,
) {
    let r = to_graphene((x - width, y - width, w + 2.0 * width, h + 2.0 * width));
    let w = width as f32;
    snapshot.append_border(
        &gsk::RoundedRect::from_rect(r, 0.0),
        &[w, w, w, w],
        &[color, color, color, color],
    );
}
