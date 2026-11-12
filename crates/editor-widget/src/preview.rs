//! The redacted preview: what core does, on the downscaled preview pixels
//! (M2 plan, "Widget"). Pure Rust, no GTK.

use image::RgbaImage;
use safe_upload_core::edit::{self, PreviewRedaction, Rect, Redaction};

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

/// The pixelate block on the preview for an area of `full_rect`: core's
/// full-size block scaled down, at least 1 px, so the preview looks as
/// coarse as the output.
pub fn preview_block(full_rect: Rect, full: (u32, u32), preview: (u32, u32)) -> u32 {
    let block = u64::from(edit::block_size(full_rect));
    let (num, den) = (u64::from(preview.0), u64::from(full.0.max(1)));
    ((block * num + den / 2) / den).max(1) as u32
}

/// The preview with every area of `model` burned in by core. No crop:
/// the editor dims outside the crop instead. Pixelate noise is seeded by
/// the area's index, so the preview does not flicker.
pub fn render(preview: &RgbaImage, model: &EditModel) -> RgbaImage {
    let full = model.size();
    let size = preview.dimensions();
    let redactions: Vec<PreviewRedaction> = model
        .areas()
        .iter()
        .enumerate()
        .map(|(i, a)| {
            let rect = to_preview(a.rect, full, size);
            PreviewRedaction {
                redaction: match a.kind {
                    AreaKind::Box => Redaction::black_box(rect),
                    AreaKind::Pixelate => Redaction::Pixelate { rect },
                },
                block: preview_block(a.rect, full, size),
                seed: i as u64,
            }
        })
        .collect();
    edit::apply_preview(preview.clone(), &redactions)
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

    /// Distinct colors per pixel, so equal neighbours mean one block.
    fn pattern(w: u32, h: u32) -> RgbaImage {
        RgbaImage::from_fn(w, h, |x, y| {
            image::Rgba([
                (x * 37 % 256) as u8,
                (y * 53 % 256) as u8,
                ((x * 11 + y * 7) % 256) as u8,
                255,
            ])
        })
    }

    /// The smallest run of equal pixels along row `y`, inside `x0..x1`,
    /// ignoring the first and last runs (they may be cut).
    fn min_run(img: &RgbaImage, y: u32, x0: u32, x1: u32) -> u32 {
        let mut edges = vec![];
        for x in x0 + 1..x1 {
            if img.get_pixel(x, y) != img.get_pixel(x - 1, y) {
                edges.push(x);
            }
        }
        edges.windows(2).map(|w| w[1] - w[0]).min().unwrap()
    }

    #[test]
    fn preview_blocks_match_the_output_blocks() {
        let mut m = EditModel::new(1000, 500);
        let rect = Rect::new(100, 100, 400, 300);
        m.add(AreaKind::Pixelate, rect);
        // The output: core at full size.
        let (_, redactions) = m.to_options();
        let out = edit::apply(pattern(1000, 500), None, &redactions).unwrap();
        let out_block = min_run(&out, 200, 100, 500);
        assert_eq!(out_block, edit::block_size(rect));
        // The preview at 1/5, measured back in full-size pixels.
        let prev = render(&pattern(200, 100), &m);
        let prev_block = min_run(&prev, 40, 20, 100) * 5;
        assert!(
            prev_block.abs_diff(out_block) <= 5,
            "preview block {prev_block} px vs output {out_block} px"
        );
        // Same model, same preview: no flicker.
        assert_eq!(render(&pattern(200, 100), &m), prev);
    }

    #[test]
    fn render_blackens_boxes_on_the_preview() {
        let mut m = EditModel::new(1000, 500);
        m.add(AreaKind::Box, Rect::new(13, 7, 101, 53));
        m.set_crop(Some(Rect::new(500, 0, 500, 500)));
        let src = RgbaImage::from_pixel(200, 100, image::Rgba([200, 200, 200, 255]));
        let out = render(&src, &m);
        assert_eq!(out.dimensions(), (200, 100));
        for (x, y, p) in out.enumerate_pixels() {
            let inside = (2..23).contains(&x) && (1..12).contains(&y);
            assert_eq!(p.0[0] == 0, inside, "pixel {x},{y}");
        }
    }
}
