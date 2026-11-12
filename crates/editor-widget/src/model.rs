//! The edit model behind the editor: areas, crop, selection and undo.
//! Pure Rust, no GTK (M2 plan, "Edit model").
//!
//! All rectangles are in full-size image pixels after orientation, the
//! coordinates core applies them in (SPEC 9.4). Redaction areas are in
//! coordinates before the crop; the crop is a separate rectangle.

use safe_upload_core::edit::{Rect, Redaction};

/// The smallest area or crop, in image pixels per side (smaller if the
/// image itself is smaller).
pub const MIN_SIZE: u32 = 4;

/// The tool picked in the toolbar.
///
/// Blur (SPEC 6.3) is added with T15 once SPEC 16.4 is answered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Tool {
    #[default]
    Box,
    Pixelate,
    Crop,
}

impl Tool {
    /// The kind of area this tool draws, or `None` for Crop.
    pub fn area_kind(self) -> Option<AreaKind> {
        match self {
            Tool::Box => Some(AreaKind::Box),
            Tool::Pixelate => Some(AreaKind::Pixelate),
            Tool::Crop => None,
        }
    }
}

/// How an area is redacted.
///
/// Blur goes here with T15 (SPEC 16.4); core has no `Redaction::Blur` yet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AreaKind {
    Box,
    Pixelate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Area {
    pub kind: AreaKind,
    pub rect: Rect,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Snapshot {
    areas: Vec<Area>,
    crop: Option<Rect>,
}

#[derive(Debug, Clone)]
pub struct EditModel {
    size: (u32, u32),
    areas: Vec<Area>,
    crop: Option<Rect>,
    selected: Option<usize>,
    undo: Vec<Snapshot>,
    redo: Vec<Snapshot>,
    /// The state when the current gesture began.
    gesture: Option<Snapshot>,
}

impl EditModel {
    /// An empty model for an image of `width` x `height` pixels after
    /// orientation.
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            size: (width, height),
            areas: Vec::new(),
            crop: None,
            selected: None,
            undo: Vec::new(),
            redo: Vec::new(),
            gesture: None,
        }
    }

    pub fn size(&self) -> (u32, u32) {
        self.size
    }

    pub fn areas(&self) -> &[Area] {
        &self.areas
    }

    pub fn crop(&self) -> Option<Rect> {
        self.crop
    }

    pub fn selected(&self) -> Option<usize> {
        self.selected
    }

    /// Selects an area. Not an edit: selection is not undone.
    pub fn select(&mut self, i: Option<usize>) {
        self.selected = i.filter(|&i| i < self.areas.len());
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty() || self.gesture.as_ref().is_some_and(|s| *s != self.snapshot())
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// Adds an area, clamped to the image, and selects it. Returns its
    /// index, or `None` if `r` is fully outside the image.
    pub fn add(&mut self, kind: AreaKind, r: Rect) -> Option<usize> {
        let rect = clamp(r, self.size)?;
        self.edit(|m| m.areas.push(Area { kind, rect }));
        self.selected = Some(self.areas.len() - 1);
        self.selected
    }

    /// Replaces the rectangle of area `i`, clamped to the image. A rect
    /// fully outside the image is ignored.
    pub fn set_rect(&mut self, i: usize, r: Rect) {
        if i < self.areas.len()
            && let Some(rect) = clamp(r, self.size)
        {
            self.edit(|m| m.areas[i].rect = rect);
        }
    }

    /// Moves area `i`, keeping its size and keeping it inside the image.
    pub fn move_by(&mut self, i: usize, dx: i32, dy: i32) {
        if let Some(a) = self.areas.get(i) {
            let rect = moved(a.rect, dx, dy, self.size);
            self.edit(|m| m.areas[i].rect = rect);
        }
    }

    /// Resizes area `i` from its bottom-right corner, keeping the minimum
    /// size and keeping it inside the image.
    pub fn resize_by(&mut self, i: usize, dw: i32, dh: i32) {
        if let Some(a) = self.areas.get(i) {
            let rect = resized(a.rect, dw, dh, self.size);
            self.edit(|m| m.areas[i].rect = rect);
        }
    }

    pub fn delete(&mut self, i: usize) {
        if i >= self.areas.len() {
            return;
        }
        self.edit(|m| {
            m.areas.remove(i);
        });
        self.selected = match self.selected {
            Some(s) if s == i => None,
            Some(s) if s > i => Some(s - 1),
            s => s,
        };
    }

    /// Sets or clears the crop, clamped to the image. A crop fully outside
    /// the image clears it.
    pub fn set_crop(&mut self, r: Option<Rect>) {
        let crop = r.and_then(|r| clamp(r, self.size));
        self.edit(|m| m.crop = crop);
    }

    /// Moves the crop, keeping it inside the image.
    pub fn move_crop_by(&mut self, dx: i32, dy: i32) {
        if let Some(c) = self.crop {
            let crop = moved(c, dx, dy, self.size);
            self.edit(|m| m.crop = Some(crop));
        }
    }

    /// Resizes the crop from its bottom-right corner.
    pub fn resize_crop_by(&mut self, dw: i32, dh: i32) {
        if let Some(c) = self.crop {
            let crop = resized(c, dw, dh, self.size);
            self.edit(|m| m.crop = Some(crop));
        }
    }

    /// Starts a gesture (a drag, or a held key). All edits until
    /// [`end_gesture`](Self::end_gesture) are one undo step.
    pub fn begin_gesture(&mut self) {
        self.end_gesture();
        self.gesture = Some(self.snapshot());
    }

    /// Ends the current gesture. Does nothing if none is running.
    pub fn end_gesture(&mut self) {
        if let Some(before) = self.gesture.take()
            && before != self.snapshot()
        {
            self.undo.push(before);
            self.redo.clear();
        }
    }

    /// Undoes the last edit. Ends a running gesture first. Returns false if
    /// there was nothing to undo.
    pub fn undo(&mut self) -> bool {
        self.end_gesture();
        let Some(s) = self.undo.pop() else {
            return false;
        };
        let now = self.restore(s);
        self.redo.push(now);
        true
    }

    /// Redoes the last undone edit. Returns false if there was nothing to
    /// redo.
    pub fn redo(&mut self) -> bool {
        self.end_gesture();
        let Some(s) = self.redo.pop() else {
            return false;
        };
        let now = self.restore(s);
        self.undo.push(now);
        true
    }

    /// The crop and redactions for `core::Options`. Rectangles are in
    /// full-size coordinates after orientation; redactions are before the
    /// crop (SPEC 9.4).
    pub fn to_options(&self) -> (Option<Rect>, Vec<Redaction>) {
        let redactions = self
            .areas
            .iter()
            .map(|a| match a.kind {
                AreaKind::Box => Redaction::black_box(a.rect),
                AreaKind::Pixelate => Redaction::Pixelate { rect: a.rect },
            })
            .collect();
        (self.crop, redactions)
    }

    fn snapshot(&self) -> Snapshot {
        Snapshot {
            areas: self.areas.clone(),
            crop: self.crop,
        }
    }

    fn restore(&mut self, s: Snapshot) -> Snapshot {
        let now = self.snapshot();
        self.areas = s.areas;
        self.crop = s.crop;
        self.select(self.selected);
        now
    }

    /// Runs one edit. Outside a gesture, a change is its own undo step and
    /// clears redo. A change that changes nothing is not recorded.
    fn edit(&mut self, f: impl FnOnce(&mut Self)) {
        let before = self.snapshot();
        f(self);
        if self.gesture.is_none() && before != self.snapshot() {
            self.undo.push(before);
            self.redo.clear();
        }
    }
}

fn min_size((w, h): (u32, u32)) -> (u32, u32) {
    (MIN_SIZE.min(w), MIN_SIZE.min(h))
}

/// Clips `r` to the image, then grows it to the minimum size. Never makes
/// the covered part of the image smaller. `None` if `r` misses the image.
fn clamp(r: Rect, size: (u32, u32)) -> Option<Rect> {
    let (mw, mh) = min_size(size);
    let (x, w) = clamp_span(r.x, r.width, size.0, mw)?;
    let (y, h) = clamp_span(r.y, r.height, size.1, mh)?;
    Some(Rect::new(x, y, w, h))
}

fn clamp_span(start: u32, len: u32, limit: u32, min: u32) -> Option<(u32, u32)> {
    let mut s = start.min(limit);
    let mut e = start.saturating_add(len).min(limit);
    if s >= e {
        return None;
    }
    if e - s < min {
        e = s + min;
        if e > limit {
            e = limit;
            s = limit - min;
        }
    }
    Some((s, e - s))
}

fn moved(r: Rect, dx: i32, dy: i32, (w, h): (u32, u32)) -> Rect {
    let shift = |p: u32, d: i32, len: u32, limit: u32| {
        (i64::from(p) + i64::from(d)).clamp(0, i64::from(limit.saturating_sub(len))) as u32
    };
    Rect::new(
        shift(r.x, dx, r.width, w),
        shift(r.y, dy, r.height, h),
        r.width,
        r.height,
    )
}

fn resized(r: Rect, dw: i32, dh: i32, size: (u32, u32)) -> Rect {
    let (mw, mh) = min_size(size);
    let grow = |len: u32, d: i32, min: u32, room: u32| {
        (i64::from(len) + i64::from(d)).clamp(i64::from(min), i64::from(room.max(min))) as u32
    };
    Rect::new(
        r.x,
        r.y,
        grow(r.width, dw, mw, size.0 - r.x),
        grow(r.height, dh, mh, size.1 - r.y),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(x: u32, y: u32, w: u32, h: u32) -> Rect {
        Rect::new(x, y, w, h)
    }

    fn model() -> EditModel {
        EditModel::new(200, 100)
    }

    fn rects(m: &EditModel) -> Vec<Rect> {
        m.areas().iter().map(|a| a.rect).collect()
    }

    #[test]
    fn add_with_undo_and_redo() {
        let mut m = model();
        assert!(!m.can_undo() && !m.undo());
        assert_eq!(m.add(AreaKind::Box, r(10, 10, 20, 20)), Some(0));
        assert_eq!(m.add(AreaKind::Pixelate, r(50, 10, 20, 20)), Some(1));
        assert_eq!(m.selected(), Some(1));
        assert!(m.undo());
        assert_eq!(rects(&m), [r(10, 10, 20, 20)]);
        assert_eq!(m.selected(), None);
        assert!(m.undo());
        assert!(m.areas().is_empty());
        assert!(!m.undo());
        assert!(m.redo() && m.redo());
        assert!(!m.redo());
        assert_eq!(m.areas()[1].kind, AreaKind::Pixelate);
        assert_eq!(rects(&m), [r(10, 10, 20, 20), r(50, 10, 20, 20)]);
    }

    #[test]
    fn move_with_undo_and_redo() {
        let mut m = model();
        m.add(AreaKind::Box, r(10, 10, 20, 20));
        m.move_by(0, 5, -3);
        assert_eq!(rects(&m), [r(15, 7, 20, 20)]);
        assert!(m.undo());
        assert_eq!(rects(&m), [r(10, 10, 20, 20)]);
        assert!(m.redo());
        assert_eq!(rects(&m), [r(15, 7, 20, 20)]);
    }

    #[test]
    fn resize_with_undo_and_redo() {
        let mut m = model();
        m.add(AreaKind::Box, r(10, 10, 20, 20));
        m.resize_by(0, 10, -5);
        assert_eq!(rects(&m), [r(10, 10, 30, 15)]);
        m.set_rect(0, r(0, 0, 8, 8));
        assert_eq!(rects(&m), [r(0, 0, 8, 8)]);
        assert!(m.undo());
        assert_eq!(rects(&m), [r(10, 10, 30, 15)]);
        assert!(m.undo());
        assert_eq!(rects(&m), [r(10, 10, 20, 20)]);
        assert!(m.redo());
        assert_eq!(rects(&m), [r(10, 10, 30, 15)]);
    }

    #[test]
    fn delete_with_undo_and_redo() {
        let mut m = model();
        m.add(AreaKind::Box, r(10, 10, 20, 20));
        m.add(AreaKind::Pixelate, r(50, 10, 20, 20));
        m.add(AreaKind::Box, r(90, 10, 20, 20));
        m.delete(0);
        assert_eq!(m.selected(), Some(1));
        assert_eq!(rects(&m), [r(50, 10, 20, 20), r(90, 10, 20, 20)]);
        m.delete(1);
        assert_eq!(m.selected(), None);
        m.delete(5);
        assert!(m.undo());
        assert!(m.undo());
        assert_eq!(rects(&m).len(), 3);
        assert_eq!(m.areas()[0].rect, r(10, 10, 20, 20));
        assert!(m.redo());
        assert_eq!(rects(&m), [r(50, 10, 20, 20), r(90, 10, 20, 20)]);
    }

    #[test]
    fn crop_with_undo_and_redo() {
        let mut m = model();
        m.set_crop(Some(r(20, 10, 100, 50)));
        m.move_crop_by(10, 0);
        m.resize_crop_by(-20, 5);
        assert_eq!(m.crop(), Some(r(30, 10, 80, 55)));
        assert!(m.undo() && m.undo());
        assert_eq!(m.crop(), Some(r(20, 10, 100, 50)));
        m.set_crop(None);
        assert_eq!(m.crop(), None);
        assert!(m.undo());
        assert_eq!(m.crop(), Some(r(20, 10, 100, 50)));
        assert!(m.undo());
        assert_eq!(m.crop(), None);
        assert!(m.redo());
        assert_eq!(m.crop(), Some(r(20, 10, 100, 50)));
    }

    #[test]
    fn new_edit_clears_redo() {
        let mut m = model();
        m.add(AreaKind::Box, r(10, 10, 20, 20));
        m.add(AreaKind::Box, r(50, 10, 20, 20));
        assert!(m.undo());
        assert!(m.can_redo());
        m.move_by(0, 1, 1);
        assert!(!m.can_redo() && !m.redo());
        assert_eq!(rects(&m), [r(11, 11, 20, 20)]);
    }

    #[test]
    fn edit_that_changes_nothing_is_not_an_undo_step() {
        let mut m = model();
        m.add(AreaKind::Box, r(0, 0, 20, 20));
        assert!(m.undo());
        assert!(m.can_redo());
        m.redo();
        m.move_by(0, -5, -5);
        assert_eq!(rects(&m), [r(0, 0, 20, 20)]);
        assert!(m.undo());
        assert!(m.areas().is_empty());
    }

    #[test]
    fn a_gesture_is_one_undo_step() {
        let mut m = model();
        m.add(AreaKind::Box, r(10, 10, 20, 20));
        m.add(AreaKind::Box, r(100, 10, 20, 20));
        m.undo();
        m.begin_gesture();
        for _ in 0..10 {
            m.move_by(0, 1, 0);
        }
        m.resize_by(0, 5, 5);
        assert!(m.can_undo() && m.can_redo());
        m.end_gesture();
        assert!(!m.can_redo());
        assert_eq!(rects(&m), [r(20, 10, 25, 25)]);
        assert!(m.undo());
        assert_eq!(rects(&m), [r(10, 10, 20, 20)]);
        assert!(m.redo());
        assert_eq!(rects(&m), [r(20, 10, 25, 25)]);

        // A gesture that ends where it began leaves no undo step.
        let mut m = model();
        m.add(AreaKind::Box, r(10, 10, 20, 20));
        m.begin_gesture();
        m.move_by(0, 5, 0);
        m.move_by(0, -5, 0);
        m.end_gesture();
        assert!(m.undo());
        assert!(!m.can_undo());

        // Undo during a gesture ends it first, then undoes it.
        let mut m = model();
        m.add(AreaKind::Box, r(10, 10, 20, 20));
        m.begin_gesture();
        m.move_by(0, 5, 0);
        assert!(m.undo());
        assert_eq!(rects(&m), [r(10, 10, 20, 20)]);
        m.end_gesture();
        assert!(m.redo());
        assert_eq!(rects(&m), [r(15, 10, 20, 20)]);
    }

    #[test]
    fn clamp_to_image() {
        let mut m = model();
        // Clipped, never shifted inward past what was drawn.
        m.add(AreaKind::Box, r(190, 90, 50, 50));
        assert_eq!(m.areas()[0].rect, r(190, 90, 10, 10));
        // Fully outside: nothing is added.
        assert_eq!(m.add(AreaKind::Box, r(200, 0, 10, 10)), None);
        assert_eq!(m.add(AreaKind::Box, r(10, 10, 0, 10)), None);
        assert_eq!(m.areas().len(), 1);
        // Minimum size, grown inside the image.
        m.add(AreaKind::Box, r(10, 10, 1, 2));
        assert_eq!(m.areas()[1].rect, r(10, 10, 4, 4));
        m.add(AreaKind::Box, r(199, 99, 1, 1));
        assert_eq!(m.areas()[2].rect, r(196, 96, 4, 4));
        // Huge rect does not overflow.
        m.add(AreaKind::Box, r(5, 5, u32::MAX, u32::MAX));
        assert_eq!(m.areas()[3].rect, r(5, 5, 195, 95));
        // Moves stop at the edges.
        m.move_by(0, -1000, 1000);
        assert_eq!(m.areas()[0].rect, r(0, 90, 10, 10));
        m.move_by(0, i32::MAX, i32::MIN);
        assert_eq!(m.areas()[0].rect, r(190, 0, 10, 10));
        // Resizes stop at the edge and at the minimum size.
        m.resize_by(0, 50, 1000);
        assert_eq!(m.areas()[0].rect, r(190, 0, 10, 100));
        m.resize_by(0, -50, i32::MIN);
        assert_eq!(m.areas()[0].rect, r(190, 0, 4, 4));
        // Crop too.
        m.set_crop(Some(r(150, 50, 100, 100)));
        assert_eq!(m.crop(), Some(r(150, 50, 50, 50)));
        m.move_crop_by(-1000, 0);
        assert_eq!(m.crop(), Some(r(0, 50, 50, 50)));
        m.set_crop(Some(r(300, 0, 10, 10)));
        assert_eq!(m.crop(), None);
    }

    #[test]
    fn clamp_on_a_tiny_image() {
        let mut m = EditModel::new(2, 3);
        m.add(AreaKind::Box, r(1, 1, 1, 1));
        assert_eq!(m.areas()[0].rect, r(0, 0, 2, 3));
        m.resize_by(0, -5, -5);
        assert_eq!(m.areas()[0].rect, r(0, 0, 2, 3));
        m.move_by(0, 3, 3);
        assert_eq!(m.areas()[0].rect, r(0, 0, 2, 3));
    }

    #[test]
    fn to_options_coords_are_pre_crop() {
        let mut m = model();
        m.set_crop(Some(r(100, 20, 80, 60)));
        m.add(AreaKind::Box, r(120, 30, 10, 10));
        m.add(AreaKind::Pixelate, r(140, 40, 20, 20));
        let (crop, redactions) = m.to_options();
        assert_eq!(crop, Some(r(100, 20, 80, 60)));
        assert_eq!(
            redactions,
            [
                Redaction::black_box(r(120, 30, 10, 10)),
                Redaction::Pixelate {
                    rect: r(140, 40, 20, 20)
                },
            ]
        );

        // Core puts the box where it was drawn, relative to the crop.
        let src = image::RgbaImage::from_pixel(200, 100, image::Rgba([200, 200, 200, 255]));
        let out = safe_upload_core::edit::apply(src, crop, &redactions[..1]).unwrap();
        assert_eq!(out.dimensions(), (80, 60));
        for (x, y, p) in out.enumerate_pixels() {
            let inside = (20..30).contains(&x) && (10..20).contains(&y);
            assert_eq!(p.0[0] == 0, inside, "pixel {x},{y}");
        }
    }

    #[test]
    fn tools_map_to_area_kinds() {
        assert_eq!(Tool::default(), Tool::Box);
        assert_eq!(Tool::Box.area_kind(), Some(AreaKind::Box));
        assert_eq!(Tool::Pixelate.area_kind(), Some(AreaKind::Pixelate));
        assert_eq!(Tool::Crop.area_kind(), None);
    }
}
