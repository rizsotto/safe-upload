//! Applies the EXIF orientation to the pixels (SPEC 9.1 step 2, FR-2).

use image::{RgbaImage, imageops};

/// EXIF orientation. The discriminants are the EXIF values 1 to 8. Each
/// value says how to transform the stored pixels to display them upright.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[repr(u16)]
pub enum Orientation {
    #[default]
    Normal = 1,
    MirrorHorizontal = 2,
    Rotate180 = 3,
    MirrorVertical = 4,
    /// Mirror horizontally, then rotate 270 degrees clockwise.
    Transpose = 5,
    Rotate90 = 6,
    /// Mirror horizontally, then rotate 90 degrees clockwise.
    Transverse = 7,
    Rotate270 = 8,
}

impl Orientation {
    /// Maps an EXIF value to an orientation. Values outside 1 to 8 are
    /// invalid in EXIF; readers treat them as normal.
    pub fn from_exif(value: u16) -> Self {
        match value {
            2 => Self::MirrorHorizontal,
            3 => Self::Rotate180,
            4 => Self::MirrorVertical,
            5 => Self::Transpose,
            6 => Self::Rotate90,
            7 => Self::Transverse,
            8 => Self::Rotate270,
            _ => Self::Normal,
        }
    }
}

/// Returns the image as it should be displayed.
pub fn apply(image: RgbaImage, orientation: Orientation) -> RgbaImage {
    match orientation {
        Orientation::Normal => image,
        Orientation::MirrorHorizontal => imageops::flip_horizontal(&image),
        Orientation::Rotate180 => imageops::rotate180(&image),
        Orientation::MirrorVertical => imageops::flip_vertical(&image),
        Orientation::Transpose => imageops::flip_horizontal(&imageops::rotate90(&image)),
        Orientation::Rotate90 => imageops::rotate90(&image),
        Orientation::Transverse => imageops::flip_horizontal(&imageops::rotate270(&image)),
        Orientation::Rotate270 => imageops::rotate270(&image),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba;

    /// 2 wide, 3 tall:
    /// a b
    /// c d
    /// e f
    fn sample() -> RgbaImage {
        RgbaImage::from_fn(2, 3, |x, y| Rgba([b'a' + (y * 2 + x) as u8, 0, 0, 255]))
    }

    fn layout(image: &RgbaImage) -> Vec<String> {
        image
            .rows()
            .map(|row| row.map(|p| p[0] as char).collect())
            .collect()
    }

    #[test]
    fn each_orientation_gives_the_expected_layout() {
        let cases: [(u16, &[&str]); 8] = [
            (1, &["ab", "cd", "ef"]),
            (2, &["ba", "dc", "fe"]),
            (3, &["fe", "dc", "ba"]),
            (4, &["ef", "cd", "ab"]),
            (5, &["ace", "bdf"]),
            (6, &["eca", "fdb"]),
            (7, &["fdb", "eca"]),
            (8, &["bdf", "ace"]),
        ];
        for (value, want) in cases {
            let o = Orientation::from_exif(value);
            assert_eq!(o as u16, value);
            assert_eq!(layout(&apply(sample(), o)), want, "orientation {value}");
        }
    }

    #[test]
    fn invalid_values_are_normal() {
        for v in [0, 9, 0xffff] {
            assert_eq!(Orientation::from_exif(v), Orientation::Normal);
        }
    }
}
