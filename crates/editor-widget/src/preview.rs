//! The redacted preview: what core does, on the downscaled preview pixels
//! (M2 plan, "Widget"). Pure Rust, no GTK.

use image::RgbaImage;
use safe_upload_core::Error;
use safe_upload_core::edit::{self, Rect, Redaction};

use crate::model::{AreaKind, EditModel};

/// The preview pixels under the full-size rect `r`, rounded outward, so
/// the preview never redacts less than the model. Exact integer math.
pub fn to_preview(r: Rect, full: (u32, u32), preview: (u32, u32)) -> Rect {
    let scale = |v: u32, num: u32, den: u32, up: bool| -> u32 {
        let (v, num, den) = (u64::from(v), u64::from(num), u64::from(den.max(1)));
        let s = if up {
            (v * num).div_ceil(den)
        } else {
            v * num / den
        };
        s.min(num) as u32
    };
    let x0 = scale(r.x, preview.0, full.0, false);
    let y0 = scale(r.y, preview.1, full.1, false);
    let x1 = scale(r.x.saturating_add(r.width), preview.0, full.0, true);
    let y1 = scale(r.y.saturating_add(r.height), preview.1, full.1, true);
    Rect::new(x0, y0, x1 - x0, y1 - y0)
}

/// The preview with every area of `model` burned in by core. No crop:
/// the editor dims outside the crop instead.
pub fn render(preview: &RgbaImage, model: &EditModel) -> Result<RgbaImage, Error> {
    let full = model.size();
    let size = preview.dimensions();
    let redactions: Vec<Redaction> = model
        .areas()
        .iter()
        .map(|a| {
            let rect = to_preview(a.rect, full, size);
            match a.kind {
                AreaKind::Box => Redaction::black_box(rect),
                AreaKind::Pixelate => Redaction::Pixelate { rect },
            }
        })
        .collect();
    edit::apply(preview.clone(), None, &redactions)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preview_rect_rounds_outward() {
        // 1000 px full, 200 px preview: 5 full px per preview px.
        let r = to_preview(Rect::new(13, 7, 101, 53), (1000, 500), (200, 100));
        // x 13..114 -> 2.6..22.8 -> 2..23; y 7..60 -> 1.4..12 -> 1..12.
        assert_eq!(r, Rect::new(2, 1, 21, 11));
        assert_eq!(
            to_preview(Rect::new(0, 0, 1000, 500), (1000, 500), (200, 100)),
            Rect::new(0, 0, 200, 100)
        );
        // Same size: unchanged.
        let r = Rect::new(3, 4, 5, 6);
        assert_eq!(to_preview(r, (50, 50), (50, 50)), r);
    }

    #[test]
    fn preview_rect_never_covers_less() {
        let mut seed = 0x1234_5677_u64;
        let mut next = |m: u32| {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            (seed % u64::from(m)) as u32
        };
        for _ in 0..20_000 {
            let full = (1 + next(5000), 1 + next(5000));
            let preview = (1 + next(full.0), 1 + next(full.1));
            let x = next(full.0);
            let y = next(full.1);
            let r = Rect::new(x, y, 1 + next(full.0 - x), 1 + next(full.1 - y));
            let p = to_preview(r, full, preview);
            let (sx, sy) = (
                f64::from(preview.0) / f64::from(full.0),
                f64::from(preview.1) / f64::from(full.1),
            );
            assert!(f64::from(p.x) <= f64::from(r.x) * sx + 1e-9);
            assert!(f64::from(p.y) <= f64::from(r.y) * sy + 1e-9);
            assert!(f64::from(p.x + p.width) >= f64::from(r.x + r.width) * sx - 1e-9);
            assert!(f64::from(p.y + p.height) >= f64::from(r.y + r.height) * sy - 1e-9);
            assert!(p.x + p.width <= preview.0 && p.y + p.height <= preview.1);
        }
    }

    #[test]
    fn render_blackens_boxes_on_the_preview() {
        let mut m = EditModel::new(1000, 500);
        m.add(AreaKind::Box, Rect::new(13, 7, 101, 53));
        m.set_crop(Some(Rect::new(500, 0, 500, 500)));
        let src = RgbaImage::from_pixel(200, 100, image::Rgba([200, 200, 200, 255]));
        let out = render(&src, &m).unwrap();
        assert_eq!(out.dimensions(), (200, 100));
        for (x, y, p) in out.enumerate_pixels() {
            let inside = (2..23).contains(&x) && (1..12).contains(&y);
            assert_eq!(p.0[0] == 0, inside, "pixel {x},{y}");
        }
    }
}
