//! Crop and redaction (SPEC 9.1 steps 3 and 4, 9.4, FR-5). Redactions are
//! burned into the pixels. Blur waits on SPEC 16.4.

use image::{Rgba, RgbaImage, imageops};

use crate::Error;

/// A rectangle in image coordinates after orientation (SPEC 9.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

impl Rect {
    pub fn new(x: u32, y: u32, width: u32, height: u32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    /// The part of `self` inside a `width` x `height` image, or `None`.
    fn clip(self, width: u32, height: u32) -> Option<Self> {
        let x1 = self.x.saturating_add(self.width).min(width);
        let y1 = self.y.saturating_add(self.height).min(height);
        (self.x < x1 && self.y < y1).then(|| Rect::new(self.x, self.y, x1 - self.x, y1 - self.y))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Redaction {
    /// Solid fill. The color is always made opaque.
    Box { rect: Rect, color: [u8; 3] },
    /// Blocks of at least 16 px and 1/40 of the short edge, plus noise.
    /// Not safe for text.
    Pixelate { rect: Rect },
}

impl Redaction {
    /// A black box, the default tool.
    pub fn black_box(rect: Rect) -> Self {
        Redaction::Box {
            rect,
            color: [0, 0, 0],
        }
    }
}

/// Noise added to each pixelate block, per channel.
pub const PIXELATE_NOISE: i16 = 8;

/// Crops, then burns in the redactions. All rectangles are in the
/// coordinates of `image` as given, before the crop.
pub fn apply(
    image: RgbaImage,
    crop: Option<Rect>,
    redactions: &[Redaction],
) -> Result<RgbaImage, Error> {
    let (mut image, origin) = match crop {
        None => (image, (0, 0)),
        Some(c) => {
            let c = c
                .clip(image.width(), image.height())
                .ok_or(Error::InvalidEdit("the crop area is outside the image"))?;
            let cropped = imageops::crop_imm(&image, c.x, c.y, c.width, c.height).to_image();
            (cropped, (c.x, c.y))
        }
    };
    let mut noise = Vec::new();
    for r in redactions {
        match *r {
            Redaction::Box { rect, color } => {
                if let Some(rect) = shift(rect, origin, &image) {
                    fill(&mut image, rect, Rgba([color[0], color[1], color[2], 255]));
                }
            }
            Redaction::Pixelate { rect } => {
                if let Some(local) = shift(rect, origin, &image) {
                    // Block size comes from the region as the user drew it.
                    let block = block_size(rect);
                    let blocks = local.width.div_ceil(block) as usize
                        * local.height.div_ceil(block) as usize;
                    noise.resize(blocks * 3, 0);
                    getrandom::fill(&mut noise)
                        .map_err(|_| Error::InvalidEdit("no random numbers for the noise"))?;
                    pixelate(&mut image, local, block, &noise);
                }
            }
        }
    }
    Ok(image)
}

/// Moves `rect` into the cropped image and clips it.
fn shift(rect: Rect, (ox, oy): (u32, u32), image: &RgbaImage) -> Option<Rect> {
    let x1 = rect.x.saturating_add(rect.width);
    let y1 = rect.y.saturating_add(rect.height);
    let x = rect.x.max(ox) - ox;
    let y = rect.y.max(oy) - oy;
    let w = x1.saturating_sub(ox).saturating_sub(x);
    let h = y1.saturating_sub(oy).saturating_sub(y);
    Rect::new(x, y, w, h).clip(image.width(), image.height())
}

fn block_size(rect: Rect) -> u32 {
    rect.width.min(rect.height).div_ceil(40).max(16)
}

fn fill(image: &mut RgbaImage, r: Rect, color: Rgba<u8>) {
    for y in r.y..r.y + r.height {
        for x in r.x..r.x + r.width {
            image.put_pixel(x, y, color);
        }
    }
}

/// Fills each block with its average color plus noise. `noise` has three
/// random bytes per block.
fn pixelate(image: &mut RgbaImage, r: Rect, block: u32, noise: &[u8]) {
    let mut noise = noise.iter();
    for by in (r.y..r.y + r.height).step_by(block as usize) {
        for bx in (r.x..r.x + r.width).step_by(block as usize) {
            let b = Rect::new(
                bx,
                by,
                block.min(r.x + r.width - bx),
                block.min(r.y + r.height - by),
            );
            let mut sum = [0u64; 4];
            for y in b.y..b.y + b.height {
                for x in b.x..b.x + b.width {
                    for (s, &c) in sum.iter_mut().zip(&image.get_pixel(x, y).0) {
                        *s += u64::from(c);
                    }
                }
            }
            let n = u64::from(b.width) * u64::from(b.height);
            let mut color = sum.map(|s| ((s + n / 2) / n) as u8);
            for c in &mut color[..3] {
                let span = (2 * PIXELATE_NOISE + 1) as u8;
                let delta = i16::from(noise.next().unwrap() % span) - PIXELATE_NOISE;
                *c = (i16::from(*c) + delta).clamp(0, 255) as u8;
            }
            fill(image, b, Rgba(color));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pattern(w: u32, h: u32) -> RgbaImage {
        RgbaImage::from_fn(w, h, |x, y| {
            Rgba([
                (x * 7 % 256) as u8,
                (y * 5 % 256) as u8,
                ((x + y) % 256) as u8,
                255,
            ])
        })
    }

    #[test]
    fn crop_size() {
        let out = apply(pattern(100, 80), Some(Rect::new(10, 20, 30, 40)), &[]).unwrap();
        assert_eq!(out.dimensions(), (30, 40));
        assert_eq!(out.get_pixel(0, 0), pattern(100, 80).get_pixel(10, 20));
        // Clipped to the image.
        let out = apply(pattern(100, 80), Some(Rect::new(90, 70, 50, 50)), &[]).unwrap();
        assert_eq!(out.dimensions(), (10, 10));
    }

    #[test]
    fn crop_outside_is_an_error() {
        for c in [Rect::new(100, 0, 5, 5), Rect::new(0, 0, 0, 5)] {
            assert!(matches!(
                apply(pattern(100, 80), Some(c), &[]),
                Err(Error::InvalidEdit(_))
            ));
        }
    }

    #[test]
    fn box_pixels_are_solid() {
        let rect = Rect::new(5, 6, 20, 10);
        let red = Redaction::Box {
            rect,
            color: [200, 0, 0],
        };
        let mut src = pattern(40, 30);
        src.put_pixel(10, 10, Rgba([1, 2, 3, 0]));
        let out = apply(src.clone(), None, &[red]).unwrap();
        for (x, y, p) in out.enumerate_pixels() {
            let inside = (5..25).contains(&x) && (6..16).contains(&y);
            if inside {
                assert_eq!(p.0, [200, 0, 0, 255]);
            } else {
                assert_eq!(p, src.get_pixel(x, y));
            }
        }
        let black = apply(src, None, &[Redaction::black_box(rect)]).unwrap();
        assert_eq!(black.get_pixel(5, 6).0, [0, 0, 0, 255]);
    }

    #[test]
    fn redactions_use_coordinates_before_the_crop() {
        let boxed = Redaction::black_box(Rect::new(30, 30, 10, 10));
        let out = apply(pattern(100, 80), Some(Rect::new(20, 20, 50, 50)), &[boxed]).unwrap();
        assert_eq!(out.get_pixel(10, 10).0, [0, 0, 0, 255]);
        assert_eq!(out.get_pixel(19, 19).0, [0, 0, 0, 255]);
        assert_ne!(out.get_pixel(20, 20).0, [0, 0, 0, 255]);
        assert_ne!(out.get_pixel(9, 9).0, [0, 0, 0, 255]);
        // A box outside the crop changes nothing.
        let outside = Redaction::black_box(Rect::new(0, 0, 10, 10));
        let plain = apply(pattern(100, 80), Some(Rect::new(20, 20, 50, 50)), &[]).unwrap();
        let out = apply(
            pattern(100, 80),
            Some(Rect::new(20, 20, 50, 50)),
            &[outside],
        )
        .unwrap();
        assert_eq!(out, plain);
    }

    #[test]
    fn pixelate_block_size() {
        assert_eq!(block_size(Rect::new(0, 0, 100, 100)), 16);
        assert_eq!(block_size(Rect::new(0, 0, 2000, 800)), 20);
        assert_eq!(block_size(Rect::new(0, 0, 801, 4000)), 21);
        assert_eq!(block_size(Rect::new(0, 0, 3, 3)), 16);
    }

    #[test]
    fn pixelate_blocks_are_uniform_with_bounded_noise() {
        let src = pattern(120, 90);
        let rect = Rect::new(4, 2, 100, 70);
        let out = apply(src.clone(), None, &[Redaction::Pixelate { rect }]).unwrap();
        let block = 16;
        for by in (2..72).step_by(block) {
            for bx in (4..104).step_by(block) {
                let x1 = (bx + 16).min(104);
                let y1 = (by + 16).min(72);
                let first = *out.get_pixel(bx, by);
                let mut sum = [0u32; 4];
                for y in by..y1 {
                    for x in bx..x1 {
                        assert_eq!(*out.get_pixel(x, y), first, "block {bx},{by}");
                        for (s, &c) in sum.iter_mut().zip(&src.get_pixel(x, y).0) {
                            *s += u32::from(c);
                        }
                    }
                }
                let n = (x1 - bx) * (y1 - by);
                for c in 0..3 {
                    let avg = (sum[c] as f64 / n as f64).round() as i32;
                    let diff = (i32::from(first[c]) - avg).abs();
                    assert!(diff <= 8, "noise {diff} in block {bx},{by}");
                }
                assert_eq!(first[3], 255);
            }
        }
        // Outside the region nothing changes.
        assert_eq!(out.get_pixel(3, 2), src.get_pixel(3, 2));
        assert_eq!(out.get_pixel(104, 50), src.get_pixel(104, 50));
        assert_eq!(out.get_pixel(50, 72), src.get_pixel(50, 72));
    }

    #[test]
    fn pixelate_adds_noise() {
        // A flat gray region: without noise every block would be 128.
        let src = RgbaImage::from_pixel(160, 160, Rgba([128, 128, 128, 255]));
        let rect = Rect::new(0, 0, 160, 160);
        let out = apply(src, None, &[Redaction::Pixelate { rect }]).unwrap();
        assert!(
            out.pixels()
                .any(|p| p[0] != 128 || p[1] != 128 || p[2] != 128)
        );
        assert!(
            out.pixels()
                .all(|p| p.0[..3].iter().all(|&c| c.abs_diff(128) <= 8))
        );
    }
}
