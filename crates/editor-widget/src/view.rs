//! Mapping between widget points and full-size image pixels.

use safe_upload_core::edit::Rect;

/// `widget = image * scale + offset`. `scale` is widget points per image
/// pixel and must be positive.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ViewTransform {
    pub scale: f64,
    pub offset: (f64, f64),
}

impl ViewTransform {
    /// Fits an image of `image` pixels into a widget of `widget` points,
    /// keeping the aspect ratio, centered.
    pub fn fit(image: (u32, u32), widget: (f64, f64)) -> Self {
        let (iw, ih) = (f64::from(image.0.max(1)), f64::from(image.1.max(1)));
        let scale = (widget.0 / iw).min(widget.1 / ih);
        let scale = if scale.is_finite() && scale > 0.0 {
            scale
        } else {
            f64::MIN_POSITIVE
        };
        Self {
            scale,
            offset: ((widget.0 - iw * scale) / 2.0, (widget.1 - ih * scale) / 2.0),
        }
    }

    pub fn to_image(&self, (x, y): (f64, f64)) -> (f64, f64) {
        (
            (x - self.offset.0) / self.scale,
            (y - self.offset.1) / self.scale,
        )
    }

    pub fn to_widget(&self, (x, y): (f64, f64)) -> (f64, f64) {
        (
            x * self.scale + self.offset.0,
            y * self.scale + self.offset.1,
        )
    }

    /// The image pixels under the widget rectangle with corners `a` and
    /// `b` (any order). Rounds outward (floor the start, ceil the end), so
    /// the result never covers less than what the user saw. Parts left of
    /// or above the image are cut at 0; the model clamps the rest.
    pub fn rect_to_image(&self, a: (f64, f64), b: (f64, f64)) -> Rect {
        let (ax, ay) = self.to_image(a);
        let (bx, by) = self.to_image(b);
        let (x0, x1) = outward(ax, bx);
        let (y0, y1) = outward(ay, by);
        Rect::new(x0, y0, x1 - x0, y1 - y0)
    }

    /// The crop for the widget rectangle with corners `a` and `b`. Rounds
    /// inward (ceil the start, floor the end), the reverse of
    /// [`rect_to_image`](Self::rect_to_image): a crop must never keep
    /// pixels the user cut away. `None` if no whole pixel is inside.
    pub fn crop_to_image(&self, a: (f64, f64), b: (f64, f64)) -> Option<Rect> {
        let (ax, ay) = self.to_image(a);
        let (bx, by) = self.to_image(b);
        let (x0, x1) = inward(ax, bx)?;
        let (y0, y1) = inward(ay, by)?;
        Some(Rect::new(x0, y0, x1 - x0, y1 - y0))
    }

    /// The widget rectangle `(x, y, width, height)` of an image rect, for
    /// drawing.
    pub fn rect_to_widget(&self, r: Rect) -> (f64, f64, f64, f64) {
        let (x, y) = self.to_widget((f64::from(r.x), f64::from(r.y)));
        (
            x,
            y,
            f64::from(r.width) * self.scale,
            f64::from(r.height) * self.scale,
        )
    }
}

/// Whole pixels covering `[min(a, b), max(a, b)]`. `as u32` saturates, and
/// NaN becomes 0.
fn outward(a: f64, b: f64) -> (u32, u32) {
    let start = a.min(b).floor() as u32;
    let end = a.max(b).ceil() as u32;
    (start, end.max(start))
}

/// Whole pixels inside `[min(a, b), max(a, b)]`, or `None`.
fn inward(a: f64, b: f64) -> Option<(u32, u32)> {
    let start = a.min(b).ceil() as u32;
    let end = a.max(b).floor() as u32;
    (end > start).then_some((start, end))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fit_centers_the_image() {
        let v = ViewTransform::fit((200, 100), (400.0, 400.0));
        assert_eq!(v.scale, 2.0);
        assert_eq!(v.offset, (0.0, 100.0));
        assert_eq!(v.to_image((200.0, 200.0)), (100.0, 50.0));
        assert_eq!(v.to_widget((100.0, 50.0)), (200.0, 200.0));
        let v = ViewTransform::fit((200, 100), (0.0, 0.0));
        assert!(v.scale > 0.0);
    }

    #[test]
    fn view_mapping_never_shrinks_a_box() {
        // Deterministic pseudo-random cases over many scales and offsets.
        let mut seed = 0x2545_f491_u64;
        let mut next = || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            (seed % 1_000_000) as f64 / 1_000_000.0
        };
        for _ in 0..20_000 {
            let v = ViewTransform {
                scale: 0.01 + next() * 8.0,
                offset: (next() * 300.0, next() * 300.0),
            };
            // Some points start before the image, where the box is cut at 0.
            let a = (
                v.offset.0 + next() * 1100.0 - 200.0,
                v.offset.1 + next() * 1100.0 - 200.0,
            );
            let b = (
                v.offset.0 + next() * 1100.0 - 200.0,
                v.offset.1 + next() * 1100.0 - 200.0,
            );
            let r = v.rect_to_image(a, b);
            let (x, y, w, h) = v.rect_to_widget(r);
            let eps = 1e-9 * (1.0 + v.scale * 1000.0);
            let lo = (a.0.min(b.0).max(v.offset.0), a.1.min(b.1).max(v.offset.1));
            assert!(x <= lo.0 + eps && y <= lo.1 + eps, "{v:?} {a:?} {b:?}");
            assert!(
                x + w >= a.0.max(b.0) - eps && y + h >= a.1.max(b.1) - eps,
                "{v:?} {a:?} {b:?}"
            );
        }
    }

    #[test]
    fn rect_rounds_outward() {
        let v = ViewTransform {
            scale: 0.5,
            offset: (10.0, 10.0),
        };
        // Image x 3.0..9.0 exactly, y 2.2..4.6 rounds to 2..5.
        let r = v.rect_to_image((14.5, 12.3), (11.5, 11.1));
        assert_eq!(r, Rect::new(3, 2, 6, 3));
        // Left of and above the image is cut at 0.
        let r = v.rect_to_image((0.0, 0.0), (12.0, 12.0));
        assert_eq!(r, Rect::new(0, 0, 4, 4));
        let r = v.rect_to_image((0.0, 0.0), (5.0, 5.0));
        assert_eq!((r.width, r.height), (0, 0));
    }

    #[test]
    fn crop_rounds_inward() {
        let v = ViewTransform {
            scale: 0.5,
            offset: (10.0, 10.0),
        };
        // Image x 3.0..9.0 stays; y 2.2..4.6 rounds in to 3..4.
        let c = v.crop_to_image((14.5, 12.3), (11.5, 11.1));
        assert_eq!(c, Some(Rect::new(3, 3, 6, 1)));
        // Image 2.2..2.8 has no whole pixel inside.
        assert_eq!(v.crop_to_image((11.1, 11.1), (11.4, 20.0)), None);
        // Left of and above the image is cut at 0.
        let c = v.crop_to_image((0.0, 0.0), (12.0, 12.0));
        assert_eq!(c, Some(Rect::new(0, 0, 4, 4)));
    }

    #[test]
    fn view_mapping_never_grows_a_crop() {
        let mut seed = 0x9e37_79b9_u64;
        let mut next = || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            (seed % 1_000_000) as f64 / 1_000_000.0
        };
        for _ in 0..20_000 {
            let v = ViewTransform {
                scale: 0.01 + next() * 8.0,
                offset: (next() * 300.0, next() * 300.0),
            };
            let a = (v.offset.0 + next() * 900.0, v.offset.1 + next() * 900.0);
            let b = (v.offset.0 + next() * 900.0, v.offset.1 + next() * 900.0);
            if let Some(c) = v.crop_to_image(a, b) {
                let (x, y, w, h) = v.rect_to_widget(c);
                let eps = 1e-9 * (1.0 + v.scale * 1000.0);
                assert!(x >= a.0.min(b.0) - eps && y >= a.1.min(b.1) - eps);
                assert!(x + w <= a.0.max(b.0) + eps && y + h <= a.1.max(b.1) + eps);
            }
        }
    }
}
