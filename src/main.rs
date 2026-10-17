mod storage;
mod ui;

use gtk::prelude::*;

fn main() -> gtk::glib::ExitCode {
    let app = gtk::Application::builder()
        .application_id("org.crystalmark.CrystalDiskInfoLinux")
        .build();

    app.connect_activate(|app| {
        ui::install_css();
        ui::build(app);
    });
    app.run()
}
