//! The drawing area inside the editor. It holds no state: it asks its
//! parent [`RedactionEditor`](crate::RedactionEditor) to draw.

use gtk::glib;
use gtk::prelude::*;
use gtk::subclass::prelude::*;

use crate::editor::RedactionEditor;

mod imp {
    use super::*;

    #[derive(Default)]
    pub struct Canvas;

    #[glib::object_subclass]
    impl ObjectSubclass for Canvas {
        const NAME: &'static str = "SafeUploadEditorCanvas";
        type Type = super::Canvas;
        type ParentType = gtk::Widget;

        fn class_init(klass: &mut Self::Class) {
            klass.set_css_name("canvas");
            klass.set_accessible_role(gtk::AccessibleRole::Img);
        }
    }

    impl ObjectImpl for Canvas {
        fn constructed(&self) {
            self.parent_constructed();
            let obj = self.obj();
            obj.set_focusable(true);
            obj.set_hexpand(true);
            obj.set_vexpand(true);
            obj.update_property(&[gtk::accessible::Property::Label("Image to redact")]);
        }
    }

    impl WidgetImpl for Canvas {
        fn measure(&self, orientation: gtk::Orientation, _for_size: i32) -> (i32, i32, i32, i32) {
            let natural = match orientation {
                gtk::Orientation::Horizontal => 480,
                _ => 360,
            };
            (64, natural, -1, -1)
        }

        fn snapshot(&self, snapshot: &gtk::Snapshot) {
            let obj = self.obj();
            if let Some(editor) = obj.parent().and_downcast::<RedactionEditor>() {
                editor.draw_canvas(snapshot);
            }
        }
    }
}

glib::wrapper! {
    pub struct Canvas(ObjectSubclass<imp::Canvas>)
        @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl Canvas {
    pub fn new() -> Self {
        glib::Object::new()
    }
}
