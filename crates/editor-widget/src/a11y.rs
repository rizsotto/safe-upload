//! The canvas's live accessible description (M2 plan, "Accessibility").

use safe_upload_core::edit::Rect;

use crate::model::{AreaKind, EditModel};

/// The warning for tools that are not safe for text (SPEC 6.3).
pub const WARNING: &str = "Not safe for text. Use Box for numbers and names.";

fn count(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

fn kind_name(k: AreaKind) -> &'static str {
    match k {
        AreaKind::Box => "box",
        AreaKind::Pixelate => "pixelate",
    }
}

fn rect_text(r: Rect) -> String {
    format!("{} by {} pixels at {}, {}", r.width, r.height, r.x, r.y)
}

/// For example: "3 areas: 2 boxes, 1 pixelate. Selected: box 2, 120 by
/// 40 pixels at 300, 210. Crop: 800 by 600 pixels at 0, 0."
pub fn describe(m: &EditModel) -> String {
    let areas = m.areas();
    let mut s = if areas.is_empty() {
        "No areas.".to_string()
    } else {
        let boxes = areas.iter().filter(|a| a.kind == AreaKind::Box).count();
        let pixelate = areas.len() - boxes;
        let mut parts = Vec::new();
        if boxes > 0 {
            parts.push(count(boxes, "box", "boxes"));
        }
        if pixelate > 0 {
            parts.push(count(pixelate, "pixelate", "pixelate"));
        }
        format!(
            "{}: {}.",
            count(areas.len(), "area", "areas"),
            parts.join(", ")
        )
    };
    if let Some(i) = m.selected() {
        let a = areas[i];
        s += &format!(
            " Selected: {} {}, {}.",
            kind_name(a.kind),
            i + 1,
            rect_text(a.rect)
        );
    }
    if let Some(c) = m.crop() {
        s += &format!(" Crop: {}.", rect_text(c));
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn describes_areas_selection_and_crop() {
        let mut m = EditModel::new(1000, 800);
        assert_eq!(describe(&m), "No areas.");
        m.add(AreaKind::Box, Rect::new(10, 10, 50, 50));
        m.select(None);
        assert_eq!(describe(&m), "1 area: 1 box.");
        m.add(AreaKind::Pixelate, Rect::new(500, 10, 50, 50));
        m.add(AreaKind::Box, Rect::new(300, 210, 120, 40));
        m.set_crop(Some(Rect::new(0, 0, 800, 600)));
        assert_eq!(
            describe(&m),
            "3 areas: 2 boxes, 1 pixelate. Selected: box 3, 120 by 40 pixels at 300, 210. \
             Crop: 800 by 600 pixels at 0, 0."
        );
    }
}
