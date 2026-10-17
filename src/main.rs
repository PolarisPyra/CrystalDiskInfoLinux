mod storage;
mod ui;

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_inner_size(ui::initial_window_size())
            .with_min_inner_size([860.0, 420.0])
            .with_title("Viper Disk Info"),
        ..Default::default()
    };

    eframe::run_native(
        "com.viper.diskinfo",
        options,
        Box::new(|context| Ok(Box::new(ui::DiskInfoApp::new(context)))),
    )
}
