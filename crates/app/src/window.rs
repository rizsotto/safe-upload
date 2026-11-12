use gettextrs::gettext;

pub fn build(app: &adw::Application) -> adw::ApplicationWindow {
    let content = adw::ToolbarView::new();
    content.add_top_bar(&adw::HeaderBar::new());

    adw::ApplicationWindow::builder()
        .application(app)
        .title(gettext("Safe Upload"))
        .default_width(800)
        .default_height(600)
        .content(&content)
        .build()
}
