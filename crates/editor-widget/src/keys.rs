//! Keyboard drawing on the canvas (M2 plan, "Keyboard-only drawing").
//! Maps a key press to an `editor.*` action. The shortcuts that work
//! anywhere in the editor (tools, undo, delete) are in
//! [`SHORTCUTS`].

use gtk::gdk;
use gtk::glib::Variant;
use gtk::prelude::ToVariant;

/// Arrow keys move by this many widget points (rounded to image pixels,
/// at least one). With `Alt`, by one image pixel.
pub const STEP_POINTS: f64 = 10.0;

/// Shortcuts on the whole editor: trigger, action, string argument.
pub const SHORTCUTS: &[(&str, &str, Option<&str>)] = &[
    ("b", "editor.tool", Some("box")),
    ("p", "editor.tool", Some("pixelate")),
    ("c", "editor.tool", Some("crop")),
    ("<Control>z", "editor.undo", None),
    ("<Control><Shift>z", "editor.redo", None),
    ("<Control>y", "editor.redo", None),
    ("Delete", "editor.delete", None),
    ("BackSpace", "editor.delete", None),
];

#[derive(Debug, Clone, PartialEq)]
pub struct KeyCommand {
    pub action: &'static str,
    pub arg: Option<Variant>,
    /// An arrow key: held presses are one undo step, until release.
    pub held: bool,
}

/// What the canvas has, for deciding what a key does.
#[derive(Debug, Clone, Copy, Default)]
pub struct KeyContext {
    /// Points per image pixel.
    pub scale: f64,
    /// Something to move: the selected area, or the crop with Crop.
    pub has_target: bool,
    /// Areas that Tab can cycle through.
    pub has_areas: bool,
}

pub fn is_arrow(key: gdk::Key) -> bool {
    arrow(key).is_some()
}

fn arrow(key: gdk::Key) -> Option<(i32, i32)> {
    match key {
        gdk::Key::Left | gdk::Key::KP_Left => Some((-1, 0)),
        gdk::Key::Right | gdk::Key::KP_Right => Some((1, 0)),
        gdk::Key::Up | gdk::Key::KP_Up => Some((0, -1)),
        gdk::Key::Down | gdk::Key::KP_Down => Some((0, 1)),
        _ => None,
    }
}

/// The action for a key press on the canvas, or `None` to let the key
/// through (so Tab still leaves the canvas when there is nothing to cycle).
pub fn command(key: gdk::Key, mods: gdk::ModifierType, ctx: KeyContext) -> Option<KeyCommand> {
    use gdk::ModifierType as M;
    let mods = mods & (M::SHIFT_MASK | M::CONTROL_MASK | M::ALT_MASK);
    let cmd = |action, arg: Option<Variant>, held| Some(KeyCommand { action, arg, held });
    if let Some((x, y)) = arrow(key) {
        if !ctx.has_target || mods.contains(M::CONTROL_MASK) {
            return None;
        }
        let step = if mods.contains(M::ALT_MASK) {
            1
        } else {
            (STEP_POINTS / ctx.scale)
                .round()
                .clamp(1.0, f64::from(i32::MAX)) as i32
        };
        let action = if mods.contains(M::SHIFT_MASK) {
            "editor.resize-by"
        } else {
            "editor.move-by"
        };
        return cmd(action, Some((x * step, y * step).to_variant()), true);
    }
    if !mods.is_empty() && !matches!(key, gdk::Key::ISO_Left_Tab | gdk::Key::Tab) {
        return None;
    }
    match key {
        gdk::Key::Return | gdk::Key::KP_Enter | gdk::Key::ISO_Enter => {
            if ctx.has_target {
                cmd("editor.commit", None, false)
            } else {
                cmd("editor.new-area", None, false)
            }
        }
        gdk::Key::Tab if mods.is_empty() && ctx.has_areas => cmd("editor.next-area", None, false),
        gdk::Key::ISO_Left_Tab | gdk::Key::Tab
            if mods == M::SHIFT_MASK || (key == gdk::Key::ISO_Left_Tab && mods.is_empty()) =>
        {
            ctx.has_areas.then(|| KeyCommand {
                action: "editor.previous-area",
                arg: None,
                held: false,
            })
        }
        gdk::Key::Escape => cmd("editor.cancel", None, false),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gdk::Key;
    use gdk::ModifierType as M;

    const CTX: KeyContext = KeyContext {
        scale: 0.5,
        has_target: true,
        has_areas: true,
    };

    fn action(key: Key, mods: M, ctx: KeyContext) -> Option<(&'static str, Option<(i32, i32)>)> {
        command(key, mods, ctx).map(|c| (c.action, c.arg.and_then(|v| v.get())))
    }

    #[test]
    fn arrows_move_and_resize() {
        let none = M::empty();
        assert_eq!(
            action(Key::Right, none, CTX),
            Some(("editor.move-by", Some((20, 0))))
        );
        assert_eq!(
            action(Key::Up, M::ALT_MASK, CTX),
            Some(("editor.move-by", Some((0, -1))))
        );
        assert_eq!(
            action(Key::Down, M::SHIFT_MASK, CTX),
            Some(("editor.resize-by", Some((0, 20))))
        );
        assert_eq!(
            action(Key::Left, M::SHIFT_MASK | M::ALT_MASK, CTX),
            Some(("editor.resize-by", Some((-1, 0))))
        );
        // Zoomed in: never less than one pixel.
        let big = KeyContext { scale: 50.0, ..CTX };
        assert_eq!(
            action(Key::Left, none, big),
            Some(("editor.move-by", Some((-1, 0))))
        );
        assert!(command(Key::Right, none, CTX).unwrap().held);
        // Nothing selected, or Ctrl: the key goes through.
        let empty = KeyContext {
            has_target: false,
            ..CTX
        };
        assert_eq!(action(Key::Right, none, empty), None);
        assert_eq!(action(Key::Right, M::CONTROL_MASK, CTX), None);
    }

    #[test]
    fn enter_tab_escape() {
        let none = M::empty();
        let empty = KeyContext {
            has_target: false,
            has_areas: false,
            ..CTX
        };
        assert_eq!(
            action(Key::Return, none, empty),
            Some(("editor.new-area", None))
        );
        assert_eq!(
            action(Key::KP_Enter, none, CTX),
            Some(("editor.commit", None))
        );
        assert_eq!(
            action(Key::Tab, none, CTX),
            Some(("editor.next-area", None))
        );
        assert_eq!(
            action(Key::ISO_Left_Tab, M::SHIFT_MASK, CTX),
            Some(("editor.previous-area", None))
        );
        // No areas: Tab leaves the canvas.
        assert_eq!(action(Key::Tab, none, empty), None);
        assert_eq!(action(Key::ISO_Left_Tab, M::SHIFT_MASK, empty), None);
        assert_eq!(
            action(Key::Escape, none, empty),
            Some(("editor.cancel", None))
        );
        assert_eq!(action(Key::Return, M::CONTROL_MASK, empty), None);
        assert_eq!(action(Key::b, none, CTX), None);
    }
}
