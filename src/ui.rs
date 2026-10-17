use crate::storage::{self, Drive};
use eframe::egui::{
    self, Align, Align2, Color32, FontId, Grid, Layout, Pos2, RichText, ScrollArea, Stroke,
    StrokeKind, Vec2,
};
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, fmt::Write as _, sync::mpsc, thread, time::Duration};

pub const MAIN_WINDOW_WIDTH: f32 = 1014.0;
pub const MAIN_WINDOW_HEIGHT: f32 = 694.0;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum RawFormat {
    #[default]
    Hex,
    Decimal,
    TwoByte,
    OneByte,
}

pub struct DiskInfoApp {
    drives: Vec<Drive>,
    selected: Option<String>,
    scan_receiver: Option<mpsc::Receiver<Vec<Drive>>>,
    loading: bool,
    scan_error: Option<String>,
    fahrenheit: bool,
    hide_serial: bool,
    dark_theme: bool,
    green_mode: bool,
    raw_format: RawFormat,
    auto_refresh_seconds: Option<f64>,
    last_scan_time: f64,
    graph_open: bool,
    temperature_history: HashMap<String, Vec<(f64, f32)>>,
    settings: UserSettings,
    temperature_dialog: bool,
    health_dialog: bool,
    settings_drive: String,
    temperature_draft: i32,
    health_draft: HealthThresholds,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
struct UserSettings {
    drives: HashMap<String, DriveSettings>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct DriveSettings {
    temperature_alarm: i32,
    threshold_05: i32,
    threshold_c5: i32,
    threshold_c6: i32,
    threshold_ff: i32,
}

#[derive(Clone, Debug)]
struct HealthThresholds {
    threshold_05: i32,
    threshold_c5: i32,
    threshold_c6: i32,
    threshold_ff: i32,
}

impl Default for HealthThresholds {
    fn default() -> Self {
        Self {
            threshold_05: 1,
            threshold_c5: 1,
            threshold_c6: 1,
            threshold_ff: 10,
        }
    }
}

impl DriveSettings {
    fn defaults(drive: &Drive) -> Self {
        let ssd = drive
            .rotation_rate
            .as_deref()
            .is_some_and(|rate| rate == "SSD" || rate == "0");
        Self {
            temperature_alarm: if ssd { 60 } else { 50 },
            threshold_05: 1,
            threshold_c5: 1,
            threshold_c6: 1,
            threshold_ff: 10,
        }
    }
}

fn settings_path() -> std::path::PathBuf {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(std::path::PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME").map(|home| std::path::PathBuf::from(home).join(".config"))
        })
        .unwrap_or_else(|| std::path::PathBuf::from("."));
    base.join("crystaldiskinfo-linux").join("settings.json")
}

fn load_settings() -> UserSettings {
    std::fs::read(settings_path())
        .ok()
        .and_then(|data| serde_json::from_slice(&data).ok())
        .unwrap_or_default()
}

fn save_settings(settings: &UserSettings) {
    let path = settings_path();
    if let (Some(parent), Ok(data)) = (path.parent(), serde_json::to_vec_pretty(settings)) {
        if std::fs::create_dir_all(parent).is_ok() {
            let _ = std::fs::write(path, data);
        }
    }
}

impl DiskInfoApp {
    pub fn new(context: &eframe::CreationContext<'_>) -> Self {
        context.egui_ctx.set_visuals(egui::Visuals::dark());
        let mut app = Self {
            drives: Vec::new(),
            selected: None,
            scan_receiver: None,
            loading: false,
            scan_error: None,
            fahrenheit: false,
            hide_serial: false,
            dark_theme: true,
            green_mode: false,
            raw_format: RawFormat::Hex,
            auto_refresh_seconds: None,
            last_scan_time: 0.0,
            graph_open: false,
            temperature_history: HashMap::new(),
            settings: load_settings(),
            temperature_dialog: false,
            health_dialog: false,
            settings_drive: String::new(),
            temperature_draft: 50,
            health_draft: HealthThresholds::default(),
        };
        app.start_scan(context.egui_ctx.clone());
        app
    }

    fn start_scan(&mut self, context: egui::Context) {
        if self.loading {
            return;
        }
        let (sender, receiver) = mpsc::channel();
        self.scan_receiver = Some(receiver);
        self.loading = true;
        self.scan_error = None;
        thread::spawn(move || {
            let _ = sender.send(storage::discover());
            context.request_repaint();
        });
    }

    fn poll_scan(&mut self, context: &egui::Context) {
        let Some(receiver) = self.scan_receiver.take() else {
            return;
        };
        match receiver.try_recv() {
            Ok(drives) => {
                let now = context.input(|input| input.time);
                for drive in &drives {
                    if let Some(temperature) = drive.temperature_c {
                        let history = self
                            .temperature_history
                            .entry(drive.name.clone())
                            .or_default();
                        history.push((now, temperature as f32));
                        if history.len() > 1440 {
                            history.remove(0);
                        }
                    }
                }
                if self
                    .selected
                    .as_ref()
                    .is_none_or(|selected| !drives.iter().any(|drive| &drive.name == selected))
                {
                    self.selected = drives.first().map(|drive| drive.name.clone());
                }
                self.drives = drives;
                self.loading = false;
                self.last_scan_time = context.input(|input| input.time);
            }
            Err(mpsc::TryRecvError::Empty) => {
                self.scan_receiver = Some(receiver);
                context.request_repaint_after(Duration::from_millis(100));
            }
            Err(mpsc::TryRecvError::Disconnected) => {
                self.loading = false;
                self.scan_error = Some("The device scan stopped unexpectedly.".into());
            }
        }
    }

    fn current_drive(&self) -> Option<&Drive> {
        self.drives
            .iter()
            .find(|drive| Some(drive.name.as_str()) == self.selected.as_deref())
    }

    fn set_interval(&mut self, seconds: Option<f64>) {
        self.auto_refresh_seconds = seconds;
        self.last_scan_time = 0.0;
    }
}

impl eframe::App for DiskInfoApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let context = ui.ctx().clone();
        self.poll_scan(&context);
        if let Some(interval) = self.auto_refresh_seconds
            && !self.loading
            && context.input(|input| input.time) - self.last_scan_time >= interval
        {
            self.start_scan(context.clone());
        }
        if context.input(|input| input.key_pressed(egui::Key::F5)) {
            self.start_scan(context.clone());
        }
        if context.input(|input| input.key_pressed(egui::Key::F6)) {
            self.start_scan(context.clone());
        }
        context.set_visuals(if self.dark_theme {
            egui::Visuals::dark()
        } else {
            egui::Visuals::light()
        });

        egui::Panel::top("main_menu")
            .resizable(false)
            .default_size(28.0)
            .frame(egui::Frame::default().inner_margin(egui::Margin::symmetric(6, 2)))
            .show(ui, |ui| self.menu_bar(ui, &context));

        egui::Panel::top("drive_tabs")
            .resizable(false)
            .default_size(50.0)
            .frame(egui::Frame::default().inner_margin(egui::Margin::symmetric(6, 4)))
            .show(ui, |ui| self.drive_tabs(ui));

        egui::CentralPanel::default()
            .frame(
                egui::Frame::central_panel(ui.style()).inner_margin(egui::Margin::symmetric(10, 8)),
            )
            .show(ui, |ui| {
                if let Some(drive) = self.current_drive() {
                    let settings = self
                        .settings
                        .drives
                        .get(&drive.name)
                        .cloned()
                        .unwrap_or_else(|| DriveSettings::defaults(drive));
                    drive_view(
                        ui,
                        drive,
                        &settings,
                        self.fahrenheit,
                        self.hide_serial,
                        self.green_mode,
                        self.raw_format,
                    );
                } else if self.loading {
                    ui.centered_and_justified(|ui| {
                        ui.spinner();
                        ui.label("Scanning drives...");
                    });
                } else if let Some(error) = &self.scan_error {
                    ui.centered_and_justified(|ui| {
                        ui.colored_label(Color32::LIGHT_RED, error);
                    });
                } else {
                    ui.centered_and_justified(|ui| ui.label("No storage devices found."));
                }
            });
        if self.graph_open {
            if let Some(drive_name) = self.selected.as_deref() {
                let title = format!("Graph - {drive_name}");
                let history = self
                    .temperature_history
                    .get(drive_name)
                    .cloned()
                    .unwrap_or_default();
                egui::Window::new(title)
                    .open(&mut self.graph_open)
                    .default_size(Vec2::new(720.0, 420.0))
                    .show(&context, |ui| {
                        temperature_graph(ui, &history, self.fahrenheit)
                    });
            }
        }
        self.settings_windows(&context);
    }
}

impl DiskInfoApp {
    fn menu_bar(&mut self, ui: &mut egui::Ui, context: &egui::Context) {
        egui::MenuBar::new().ui(ui, |ui| {
            ui.menu_button("File", |ui| {
                if ui.button("Copy report to clipboard").clicked() {
                    context.copy_text(self.report_text());
                    ui.close();
                }
                ui.separator();
                if ui.button("Exit").clicked() {
                    context.send_viewport_cmd(egui::ViewportCommand::Close);
                    ui.close();
                }
            });
            ui.menu_button("Edit", |ui| {
                if ui.button("Copy drive information").clicked() {
                    context.copy_text(self.report_text());
                    ui.close();
                }
                ui.checkbox(&mut self.hide_serial, "Hide serial number");
            });
            ui.menu_button("Function", |ui| {
                if ui.button("Refresh     F5").clicked() {
                    self.start_scan(context.clone());
                    ui.close();
                }
                ui.menu_button("Auto Refresh", |ui| {
                    for (label, interval) in [
                        ("Disable", None),
                        ("1 min.", Some(60.0)),
                        ("3 min.", Some(180.0)),
                        ("5 min.", Some(300.0)),
                        ("10 min.", Some(600.0)),
                        ("30 min.", Some(1800.0)),
                        ("60 min.", Some(3600.0)),
                        ("120 min.", Some(7200.0)),
                        ("180 min.", Some(10800.0)),
                        ("360 min.", Some(21600.0)),
                        ("720 min.", Some(43200.0)),
                        ("1440 min.", Some(86400.0)),
                    ] {
                        let selected = self.auto_refresh_seconds == interval;
                        if ui.selectable_label(selected, label).clicked() {
                            self.set_interval(interval);
                            ui.close();
                        }
                    }
                });
                ui.separator();
                if ui.button("Temperature Setting").clicked() {
                    self.begin_settings();
                    self.temperature_dialog = true;
                    ui.close();
                }
                if ui.button("Health Status Setting").clicked() {
                    self.begin_settings();
                    self.health_dialog = true;
                    ui.close();
                }
                if ui.button("Graph").clicked() {
                    self.graph_open = true;
                    ui.close();
                }
                ui.checkbox(&mut self.fahrenheit, "Fahrenheit");
                ui.checkbox(&mut self.green_mode, "Green mode");
                ui.menu_button("Raw Values", |ui| {
                    for (label, format) in [
                        ("16 [HEX]", RawFormat::Hex),
                        ("10 [DEC]", RawFormat::Decimal),
                        ("10 [DEC] - 2byte", RawFormat::TwoByte),
                        ("10 [DEC] - 1byte", RawFormat::OneByte),
                    ] {
                        if ui
                            .selectable_label(self.raw_format == format, label)
                            .clicked()
                        {
                            self.raw_format = format;
                            ui.close();
                        }
                    }
                });
            });
            ui.menu_button("Theme", |ui| {
                if ui.selectable_label(self.dark_theme, "Dark").clicked() {
                    self.dark_theme = true;
                    ui.close();
                }
                if ui.selectable_label(!self.dark_theme, "Light").clicked() {
                    self.dark_theme = false;
                    ui.close();
                }
            });
            ui.menu_button("Disk", |ui| {
                for drive in &self.drives {
                    if ui
                        .selectable_label(
                            self.selected.as_deref() == Some(&drive.name),
                            &drive.name,
                        )
                        .clicked()
                    {
                        self.selected = Some(drive.name.clone());
                        ui.close();
                    }
                }
            });
            ui.menu_button("Help", |ui| {
                ui.label("CrystalDiskInfo Linux port");
                ui.label("SMART data is provided by smartmontools.");
            });
            if self.loading {
                ui.spinner();
                ui.label("Scanning");
            }
        });
    }

    fn begin_settings(&mut self) {
        let Some(drive) = self.current_drive().cloned() else {
            return;
        };
        self.settings_drive = drive.name.clone();
        let settings = self
            .settings
            .drives
            .get(&drive.name)
            .cloned()
            .unwrap_or_else(|| DriveSettings::defaults(&drive));
        self.temperature_draft = settings.temperature_alarm;
        self.health_draft = HealthThresholds {
            threshold_05: settings.threshold_05,
            threshold_c5: settings.threshold_c5,
            threshold_c6: settings.threshold_c6,
            threshold_ff: settings.threshold_ff,
        };
    }

    fn settings_windows(&mut self, context: &egui::Context) {
        let drive_index = self
            .drives
            .iter()
            .position(|drive| drive.name == self.settings_drive);
        if self.temperature_dialog {
            let mut open = self.temperature_dialog;
            egui::Window::new("Temperature Setting")
                .open(&mut open)
                .collapsible(false)
                .resizable(false)
                .show(context, |ui| {
                    if let Some(index) = drive_index {
                        egui::ComboBox::from_label("Drive")
                            .selected_text(format!(
                                "{} ({})",
                                self.drives[index].model, self.drives[index].name
                            ))
                            .show_ui(ui, |ui| {
                                for drive in &self.drives {
                                    if ui
                                        .selectable_label(
                                            drive.name == self.settings_drive,
                                            format!("{} ({})", drive.model, drive.name),
                                        )
                                        .clicked()
                                    {
                                        self.settings_drive = drive.name.clone();
                                        let saved = self
                                            .settings
                                            .drives
                                            .get(&drive.name)
                                            .cloned()
                                            .unwrap_or_else(|| DriveSettings::defaults(drive));
                                        self.temperature_draft = saved.temperature_alarm;
                                    }
                                }
                            });
                        ui.horizontal(|ui| {
                            ui.label("Alarm temperature");
                            ui.add(
                                egui::Slider::new(&mut self.temperature_draft, 10..=90)
                                    .suffix(" °C"),
                            );
                        });
                        ui.label(format!(
                            "{} °C / {} °F",
                            self.temperature_draft,
                            self.temperature_draft * 9 / 5 + 32
                        ));
                        ui.horizontal(|ui| {
                            if ui.button("Default").clicked() {
                                self.temperature_draft =
                                    DriveSettings::defaults(&self.drives[index]).temperature_alarm;
                            }
                            if ui.button("Apply").clicked() {
                                let drive = &self.drives[index];
                                let entry = self
                                    .settings
                                    .drives
                                    .entry(drive.name.clone())
                                    .or_insert_with(|| DriveSettings::defaults(drive));
                                entry.temperature_alarm = self.temperature_draft;
                                save_settings(&self.settings);
                            }
                        });
                    } else {
                        ui.label("No drive selected.");
                    }
                });
            self.temperature_dialog = open;
        }
        if self.health_dialog {
            let mut open = self.health_dialog;
            egui::Window::new("Health Status Setting")
                .open(&mut open)
                .collapsible(false)
                .resizable(false)
                .show(context, |ui| {
                    if let Some(index) = drive_index {
                        egui::ComboBox::from_label("Drive")
                            .selected_text(format!(
                                "{} ({})",
                                self.drives[index].model, self.drives[index].name
                            ))
                            .show_ui(ui, |ui| {
                                for drive in &self.drives {
                                    if ui
                                        .selectable_label(
                                            drive.name == self.settings_drive,
                                            format!("{} ({})", drive.model, drive.name),
                                        )
                                        .clicked()
                                    {
                                        self.settings_drive = drive.name.clone();
                                        let saved = self
                                            .settings
                                            .drives
                                            .get(&drive.name)
                                            .cloned()
                                            .unwrap_or_else(|| DriveSettings::defaults(drive));
                                        self.health_draft = HealthThresholds {
                                            threshold_05: saved.threshold_05,
                                            threshold_c5: saved.threshold_c5,
                                            threshold_c6: saved.threshold_c6,
                                            threshold_ff: saved.threshold_ff,
                                        };
                                    }
                                }
                            });
                        let ssd = self.drives[index]
                            .rotation_rate
                            .as_deref()
                            .is_some_and(|rate| rate == "SSD" || rate == "0");
                        if ssd {
                            if self.drives[index].health_percent.is_some() {
                                ui.add(
                                    egui::Slider::new(&mut self.health_draft.threshold_ff, 0..=99)
                                        .text("ID FF wear threshold"),
                                );
                            } else {
                                ui.label("Wear threshold is unavailable for this SSD.");
                            }
                        } else {
                            ui.add(
                                egui::Slider::new(&mut self.health_draft.threshold_05, 0..=255)
                                    .text("ID 05 threshold"),
                            );
                            ui.add(
                                egui::Slider::new(&mut self.health_draft.threshold_c5, 0..=255)
                                    .text("ID C5 threshold"),
                            );
                            ui.add(
                                egui::Slider::new(&mut self.health_draft.threshold_c6, 0..=255)
                                    .text("ID C6 threshold"),
                            );
                        }
                        ui.horizontal(|ui| {
                            if ui.button("Default").clicked() {
                                self.health_draft = HealthThresholds::default();
                            }
                            if ui.button("Apply").clicked() {
                                let drive = &self.drives[index];
                                let entry = self
                                    .settings
                                    .drives
                                    .entry(drive.name.clone())
                                    .or_insert_with(|| DriveSettings::defaults(drive));
                                entry.threshold_05 = self.health_draft.threshold_05;
                                entry.threshold_c5 = self.health_draft.threshold_c5;
                                entry.threshold_c6 = self.health_draft.threshold_c6;
                                entry.threshold_ff = self.health_draft.threshold_ff;
                                save_settings(&self.settings);
                            }
                        });
                    } else {
                        ui.label("No drive selected.");
                    }
                });
            self.health_dialog = open;
        }
    }

    fn drive_tabs(&mut self, ui: &mut egui::Ui) {
        ScrollArea::horizontal()
            .auto_shrink([false, true])
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    for (index, drive) in self.drives.iter().enumerate() {
                        let selected = self.selected.as_deref() == Some(drive.name.as_str());
                        let temperature = drive
                            .temperature_c
                            .map(|value| show_temperature(value, self.fahrenheit))
                            .unwrap_or_else(|| "--".into());
                        let settings = self
                            .settings
                            .drives
                            .get(&drive.name)
                            .cloned()
                            .unwrap_or_else(|| DriveSettings::defaults(drive));
                        let status_color =
                            status_label_with_settings(drive, &settings, self.green_mode).1;
                        let button = egui::Button::new(
                            RichText::new(format!("Disk{index}\n{temperature}"))
                                .size(12.0)
                                .color(status_color),
                        )
                        .min_size(Vec2::new(48.0, 40.0));
                        if ui.add(button.selected(selected)).clicked() {
                            self.selected = Some(drive.name.clone());
                        }
                    }
                    if self.drives.is_empty() && !self.loading {
                        ui.label("No detected disks");
                    }
                });
            });
    }

    fn report_text(&self) -> String {
        let mut report = String::from("CrystalDiskInfo\n");
        if let Some(drive) = self.current_drive() {
            let _ = writeln!(report, "Device: /dev/{}", drive.name);
            let _ = writeln!(report, "Model: {}", drive.model);
            if !self.hide_serial {
                let _ = writeln!(report, "Serial number: {}", drive.serial);
            }
            let _ = writeln!(report, "Health: {}", drive.assessment);
            let _ = writeln!(
                report,
                "Temperature: {}",
                drive
                    .temperature_c
                    .map(|value| show_temperature(value, self.fahrenheit))
                    .unwrap_or_else(|| "Unavailable".into())
            );
            let _ = writeln!(
                report,
                "Capacity: {}",
                storage::format_capacity(drive.capacity)
            );
            for attribute in &drive.ata_attributes {
                let _ = writeln!(
                    report,
                    "{} {}: {}",
                    attribute.id, attribute.name, attribute.raw
                );
            }
        }
        report
    }
}

fn drive_view(
    ui: &mut egui::Ui,
    drive: &Drive,
    settings: &DriveSettings,
    fahrenheit: bool,
    hide_serial: bool,
    green_mode: bool,
    raw_format: RawFormat,
) {
    ui.vertical(|ui| {
        let (status, color) = status_label_with_settings(drive, settings, green_mode);
        let model_capacity = format!(
            "{} : {}",
            drive.model,
            storage::format_capacity(drive.capacity)
        );
        ui.label(RichText::new(model_capacity).strong().size(16.0));
        ui.add_space(4.0);

        ui.allocate_ui_with_layout(
            Vec2::new(ui.available_width(), 192.0),
            Layout::left_to_right(Align::Min),
            |ui| {
                ui.allocate_ui_with_layout(
                    Vec2::new(112.0, 192.0),
                    Layout::top_down(Align::Min),
                    |ui| {
                        ui.label(RichText::new("Health Status").weak().size(11.0));
                        egui::Frame::group(ui.style()).show(ui, |ui| {
                            ui.set_min_size(Vec2::new(100.0, 34.0));
                            ui.centered_and_justified(|ui| {
                                ui.colored_label(color, RichText::new(status).strong());
                            });
                        });
                        ui.add_space(8.0);
                        ui.label(RichText::new("Temperature").weak().size(11.0));
                        egui::Frame::group(ui.style()).show(ui, |ui| {
                            ui.set_min_size(Vec2::new(100.0, 34.0));
                            ui.centered_and_justified(|ui| {
                                let temperature = drive
                                    .temperature_c
                                    .map(|value| show_temperature(value, fahrenheit))
                                    .unwrap_or_else(|| {
                                        if fahrenheit { "-- °F" } else { "-- °C" }.into()
                                    });
                                let temperature_color = drive
                                    .temperature_c
                                    .map(|temperature| {
                                        if temperature >= i64::from(settings.temperature_alarm) {
                                            Color32::from_rgb(234, 77, 77)
                                        } else {
                                            color
                                        }
                                    })
                                    .unwrap_or(color);
                                ui.colored_label(
                                    temperature_color,
                                    RichText::new(temperature).size(18.0).strong(),
                                );
                            });
                        });
                        if let Some(life) = drive.health_percent {
                            ui.add_space(8.0);
                            ui.label(RichText::new("Life").weak().size(11.0));
                            ui.add(
                                egui::ProgressBar::new(f32::from(life) / 100.0)
                                    .text(format!("{life}%")),
                            );
                        }
                    },
                );

                ui.separator();
                ui.columns(2, |columns| {
                    metadata_column(&mut columns[0], drive, hide_serial, fahrenheit);
                    metadata_column_secondary(&mut columns[1], drive);
                });
            },
        );

        ui.separator();
        if !drive.ata_attributes.is_empty() {
            ui.label(RichText::new("S.M.A.R.T.").strong().size(12.0));
            ata_attribute_table(ui, drive, settings, raw_format);
        } else if !drive.nvme_metrics.is_empty() {
            ui.label(RichText::new("NVMe S.M.A.R.T.").strong().size(12.0));
            nvme_table(ui, drive);
        } else {
            ui.label(
                RichText::new(
                    drive
                        .note
                        .as_deref()
                        .unwrap_or("S.M.A.R.T. data is unavailable for this drive."),
                )
                .weak(),
            );
        }
    });
}

fn metadata_column(ui: &mut egui::Ui, drive: &Drive, hide_serial: bool, fahrenheit: bool) {
    Grid::new("metadata_left")
        .num_columns(2)
        .spacing(Vec2::new(12.0, 6.0))
        .show(ui, |ui| {
            metadata(ui, "Firmware", option(&drive.firmware));
            metadata(
                ui,
                "Serial Number",
                if hide_serial {
                    "Hidden"
                } else {
                    available(&drive.serial)
                },
            );
            metadata(ui, "Interface", option(&drive.interface));
            metadata(ui, "Transfer Mode", option(&drive.transfer_mode));
            metadata(ui, "Drive Letter", &drive.drive_map.join(", "));
            metadata(ui, "Standard", &drive.kind);
            metadata(
                ui,
                "Temperature",
                &drive
                    .temperature_c
                    .map(|value| show_temperature(value, fahrenheit))
                    .unwrap_or_else(|| "Unavailable".into()),
            );
        });
}

fn metadata_column_secondary(ui: &mut egui::Ui, drive: &Drive) {
    Grid::new("metadata_right")
        .num_columns(2)
        .spacing(Vec2::new(12.0, 6.0))
        .show(ui, |ui| {
            metadata(ui, "Buffer Size", "Unavailable");
            metadata(ui, "NV Cache Size", "Unavailable");
            metadata(ui, "Rotation Rate", option(&drive.rotation_rate));
            metadata(
                ui,
                "Power On Count",
                &drive
                    .power_cycles
                    .map(|value| value.to_string())
                    .unwrap_or_else(|| "Unavailable".into()),
            );
            metadata(
                ui,
                "Power On Hours",
                &drive
                    .power_on_hours
                    .map(|value| format!("{value} hours"))
                    .unwrap_or_else(|| "Unavailable".into()),
            );
            metadata(ui, "Features", "");
            metadata(
                ui,
                "SMART Access",
                if drive.smart_available {
                    "Enabled"
                } else {
                    "Unavailable"
                },
            );
        });
}

fn metadata(ui: &mut egui::Ui, label: &str, value: &str) {
    ui.label(RichText::new(label).weak());
    ui.label(value);
    ui.end_row();
}

fn ata_attribute_table(
    ui: &mut egui::Ui,
    drive: &Drive,
    settings: &DriveSettings,
    raw_format: RawFormat,
) {
    let headers = [
        "",
        "ID",
        "Attribute Name",
        "Current",
        "Worst",
        "Threshold",
        "Raw Values",
    ];
    ScrollArea::vertical()
        .auto_shrink([false, true])
        .max_height(ui.available_height())
        .show(ui, |ui| {
            Grid::new("ata_smart_table")
                .num_columns(headers.len())
                .spacing(Vec2::new(10.0, 3.0))
                .striped(true)
                .show(ui, |ui| {
                    for heading in headers {
                        ui.label(RichText::new(heading).weak().size(11.0));
                    }
                    ui.end_row();
                    for attribute in &drive.ata_attributes {
                        let threshold = match attribute.id {
                            0x05 => settings.threshold_05,
                            0xC5 => settings.threshold_c5,
                            0xC6 => settings.threshold_c6,
                            _ => 0,
                        };
                        let caution = threshold > 0
                            && attribute
                                .raw_value
                                .is_some_and(|raw| raw >= threshold as u64);
                        let failed = attribute.failed
                            || (attribute.threshold > 0 && attribute.current < attribute.threshold);
                        let (label, color) = if failed {
                            ("BAD", Color32::from_rgb(234, 77, 77))
                        } else if caution {
                            ("CAUTION", Color32::from_rgb(224, 170, 55))
                        } else {
                            ("GOOD", Color32::from_rgb(63, 184, 107))
                        };
                        ui.colored_label(color, label);
                        ui.label(format!("{:02X}", attribute.id));
                        ui.label(&attribute.name);
                        ui.label(attribute.current.to_string());
                        ui.label(attribute.worst.to_string());
                        ui.label(if attribute.threshold == 0 {
                            "---".into()
                        } else {
                            attribute.threshold.to_string()
                        });
                        ui.label(format_raw(attribute, raw_format));
                        ui.end_row();
                    }
                });
        });
}

fn format_raw(attribute: &storage::AtaSmartAttribute, format: RawFormat) -> String {
    let Some(value) = attribute.raw_value else {
        return attribute.raw.clone();
    };
    let byte = |index: u32| ((value >> (index * 8)) & 0xff) as u8;
    match format {
        RawFormat::Hex => format!(
            "00{:02X}{:02X}{:02X}{:02X}{:02X}{:02X}",
            byte(5),
            byte(4),
            byte(3),
            byte(2),
            byte(1),
            byte(0)
        ),
        RawFormat::Decimal => value.to_string(),
        RawFormat::TwoByte => format!(
            "{} {} {}",
            u16::from_le_bytes([byte(4), byte(5)]),
            u16::from_le_bytes([byte(2), byte(3)]),
            u16::from_le_bytes([byte(0), byte(1)])
        ),
        RawFormat::OneByte => format!(
            "{} {} {} {} {} {}",
            byte(5),
            byte(4),
            byte(3),
            byte(2),
            byte(1),
            byte(0)
        ),
    }
}

fn nvme_table(ui: &mut egui::Ui, drive: &Drive) {
    Grid::new("nvme_smart_table")
        .num_columns(4)
        .spacing(Vec2::new(10.0, 3.0))
        .striped(true)
        .show(ui, |ui| {
            for heading in ["", "ID", "Attribute Name", "Raw Values"] {
                ui.label(RichText::new(heading).weak().size(11.0));
            }
            ui.end_row();
            for metric in &drive.nvme_metrics {
                let (label, color) = if drive.smart_status == Some(false) {
                    ("BAD", Color32::from_rgb(234, 77, 77))
                } else if drive.smart_status == Some(true) {
                    ("GOOD", Color32::from_rgb(63, 184, 107))
                } else {
                    ("UNKNOWN", Color32::GRAY)
                };
                ui.colored_label(color, label);
                ui.label("");
                ui.label(&metric.name);
                ui.label(&metric.value);
                ui.end_row();
            }
        });
}

fn status_label(drive: &Drive, green_mode: bool) -> (&'static str, Color32) {
    match drive.smart_status {
        Some(false) => ("Bad", Color32::from_rgb(234, 77, 77)),
        Some(true) if green_mode => ("Good", Color32::from_rgb(87, 205, 95)),
        Some(true) => ("Good", Color32::from_rgb(55, 181, 118)),
        None => ("Unknown", Color32::from_rgb(145, 150, 160)),
    }
}

fn status_label_with_settings(
    drive: &Drive,
    settings: &DriveSettings,
    green_mode: bool,
) -> (&'static str, Color32) {
    let mut caution = false;
    let ssd = drive
        .rotation_rate
        .as_deref()
        .is_some_and(|rate| rate == "SSD" || rate == "0");
    for attribute in &drive.ata_attributes {
        let threshold = if ssd && attribute.id == 0x05 {
            settings.threshold_ff
        } else {
            match attribute.id {
                0x05 => settings.threshold_05,
                0xC5 => settings.threshold_c5,
                0xC6 => settings.threshold_c6,
                _ => 0,
            }
        };
        if threshold > 0
            && attribute.raw_value.is_some_and(|raw| {
                if ssd && attribute.id == 0x05 {
                    100u64.saturating_sub(raw.min(100)) <= threshold as u64
                } else {
                    raw >= threshold as u64
                }
            })
        {
            caution = true;
        }
    }
    let (base_status, base_color) = status_label(drive, green_mode);
    if drive
        .temperature_c
        .is_some_and(|temperature| temperature >= i64::from(settings.temperature_alarm))
        && base_status != "Bad"
    {
        return ("Bad", Color32::from_rgb(234, 77, 77));
    }
    if base_status == "Good" && caution {
        ("Caution", Color32::from_rgb(224, 170, 55))
    } else {
        (base_status, base_color)
    }
}

fn show_temperature(celsius: i64, fahrenheit: bool) -> String {
    if fahrenheit {
        format!("{} °F", celsius * 9 / 5 + 32)
    } else {
        format!("{celsius} °C")
    }
}

fn temperature_graph(ui: &mut egui::Ui, history: &[(f64, f32)], fahrenheit: bool) {
    ui.label(RichText::new("TEMPERATURE HISTORY").strong());
    ui.label("Samples are recorded each time the drive scan completes.");
    ui.add_space(8.0);
    let (rect, _) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), 280.0), egui::Sense::hover());
    let painter = ui.painter_at(rect);
    let panel = ui.visuals().extreme_bg_color;
    painter.rect_filled(rect, 4.0, panel);
    painter.rect_stroke(
        rect,
        4.0,
        Stroke::new(1.0, ui.visuals().widgets.noninteractive.bg_stroke.color),
        StrokeKind::Inside,
    );
    if history.is_empty() {
        painter.text(
            rect.center(),
            Align2::CENTER_CENTER,
            "No temperature history yet. Refresh the drive to record a sample.",
            FontId::proportional(14.0),
            ui.visuals().text_color(),
        );
        return;
    }

    let plot = rect.shrink2(Vec2::new(48.0, 26.0));
    let temperatures = history
        .iter()
        .map(|(_, value)| {
            if fahrenheit {
                value * 9.0 / 5.0 + 32.0
            } else {
                *value
            }
        })
        .collect::<Vec<_>>();
    let min = temperatures
        .iter()
        .copied()
        .fold(f32::INFINITY, f32::min)
        .floor()
        - 2.0;
    let max = temperatures
        .iter()
        .copied()
        .fold(f32::NEG_INFINITY, f32::max)
        .ceil()
        + 2.0;
    let span = (max - min).max(1.0);
    for step in 0..=4 {
        let fraction = step as f32 / 4.0;
        let y = plot.bottom() - fraction * plot.height();
        painter.line_segment(
            [Pos2::new(plot.left(), y), Pos2::new(plot.right(), y)],
            Stroke::new(1.0, ui.visuals().widgets.noninteractive.bg_stroke.color),
        );
        painter.text(
            Pos2::new(rect.left() + 4.0, y),
            Align2::LEFT_CENTER,
            format!("{:.0}°", min + span * fraction),
            FontId::proportional(11.0),
            ui.visuals().weak_text_color(),
        );
    }
    let first_time = history.first().map(|sample| sample.0).unwrap_or_default();
    let last_time = history.last().map(|sample| sample.0).unwrap_or(first_time);
    let time_span = (last_time - first_time).max(1.0);
    let points = history
        .iter()
        .zip(temperatures.iter())
        .map(|((time, _), temperature)| {
            let x = plot.left() + ((*time - first_time) / time_span) as f32 * plot.width();
            let y = plot.bottom() - (*temperature - min) / span * plot.height();
            Pos2::new(x, y)
        })
        .collect::<Vec<_>>();
    for pair in points.windows(2) {
        painter.line_segment(
            [pair[0], pair[1]],
            Stroke::new(2.0, Color32::from_rgb(80, 180, 235)),
        );
    }
    if let Some(point) = points.last() {
        painter.circle_filled(*point, 4.0, Color32::from_rgb(80, 180, 235));
    }
    painter.text(
        Pos2::new(rect.center().x, rect.bottom() - 8.0),
        Align2::CENTER_BOTTOM,
        format!("{} samples", history.len()),
        FontId::proportional(11.0),
        ui.visuals().weak_text_color(),
    );
}

fn option(value: &Option<String>) -> &str {
    value
        .as_deref()
        .filter(|value| !value.is_empty())
        .unwrap_or("Unavailable")
}

fn available(value: &str) -> &str {
    if value.is_empty() {
        "Unavailable"
    } else {
        value
    }
}
