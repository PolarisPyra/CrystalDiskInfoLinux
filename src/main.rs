mod storage;
mod ui;

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_inner_size([1120.0, 780.0])
            .with_min_inner_size([860.0, 600.0])
            .with_title("CrystalDiskInfo for Linux"),
        ..Default::default()
    };

    eframe::run_native(
        "org.crystalmark.CrystalDiskInfoLinux",
        options,
        Box::new(|context| Ok(Box::new(ui::DiskInfoApp::new(context)))),
    )
}
