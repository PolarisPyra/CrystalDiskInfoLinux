use crate::storage::{self, Drive};
use eframe::egui::{self, Align, Color32, Grid, Layout, RichText, ScrollArea, Vec2};
use std::{sync::mpsc, thread, time::Duration};

pub struct DiskInfoApp {
    drives: Vec<Drive>,
    selected: Option<String>,
    scan_receiver: Option<mpsc::Receiver<Vec<Drive>>>,
    loading: bool,
    scan_error: Option<String>,
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
                self.selected = drives.first().map(|drive| drive.name.clone());
                self.drives = drives;
                self.loading = false;
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
}

impl eframe::App for DiskInfoApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let context = ui.ctx().clone();
        self.poll_scan(&context);

        egui::Panel::top("app_header")
            .default_size(78.0)
            .frame(egui::Frame::default().inner_margin(egui::Margin::symmetric(22, 16)))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.heading("CrystalDiskInfo");
                        ui.label(RichText::new("Storage health monitor").weak());
                    });
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let refresh = ui.add_enabled(!self.loading, egui::Button::new("Refresh"));
                        if refresh.clicked() {
                            self.start_scan(context.clone());
                        }
                        if self.loading {
                            ui.spinner();
                            ui.label("Scanning drives");
                        }
                    });
                });
            });

        egui::Panel::left("device_list")
            .resizable(true)
            .default_size(300.0)
            .min_size(220.0)
            .max_size(400.0)
            .frame(egui::Frame::side_top_panel(ui.style()).inner_margin(16))
            .show(ui, |ui| {
                ui.label(RichText::new("STORAGE DEVICES").strong().weak());
                ui.add_space(10.0);
                if let Some(error) = &self.scan_error {
                    ui.colored_label(Color32::LIGHT_RED, error);
                } else if self.drives.is_empty() && !self.loading {
                    ui.label("No drives found.");
                }
                ScrollArea::vertical().show(ui, |ui| {
                    for drive in &self.drives {
                        let selected = self.selected.as_deref() == Some(drive.name.as_str());
                        let status = match drive.smart_status {
                            Some(true) => "Good",
                            Some(false) => "Warning",
                            None => "Unknown",
                        };
                        let response = ui.selectable_label(
                            selected,
                            format!(
                                "{}\n{}  |  {}\n{}  |  {}",
                                drive.model,
                                drive.name,
                                storage::format_capacity(drive.capacity),
                                drive.kind,
                                status,
                            ),
                        );
                        if response.clicked() {
                            self.selected = Some(drive.name.clone());
                        }
                        ui.add_space(4.0);
                    }
                });
            });

        egui::CentralPanel::default()
            .frame(egui::Frame::central_panel(ui.style()).inner_margin(22))
            .show(ui, |ui| {
                if let Some(drive) = self
                    .drives
                    .iter()
                    .find(|drive| Some(drive.name.as_str()) == self.selected.as_deref())
                {
                    drive_details(ui, drive);
                } else if self.loading {
                    ui.centered_and_justified(|ui| {
                        ui.spinner();
                        ui.label("Scanning Linux block devices and reading SMART data...");
                    });
                } else {
                    ui.centered_and_justified(|ui| {
                        ui.label("No storage device is selected.");
                    });
                }
            });
    }
}

fn drive_details(ui: &mut egui::Ui, drive: &Drive) {
    ScrollArea::vertical().show(ui, |ui| {
        ui.label(RichText::new("DRIVE HEALTH").strong().weak());
        ui.add_space(8.0);
        egui::Frame::group(ui.style()).show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            let status_color = match drive.smart_status {
                Some(true) => Color32::from_rgb(75, 190, 125),
                Some(false) => Color32::from_rgb(240, 95, 85),
                None => Color32::from_rgb(220, 175, 75),
            };
            ui.colored_label(
                status_color,
                RichText::new(if drive.assessment.is_empty() {
                    "Health unavailable"
                } else {
                    &drive.assessment
                })
                .size(28.0)
                .strong(),
            );
            ui.label(drive.note.as_deref().unwrap_or(if drive.smart_available {
                "Health and attributes reported by smartctl."
            } else {
                "Install smartmontools and allow device access to read SMART information."
            }));
            if let Some(remaining) = drive.health_percent {
                ui.add_space(8.0);
                ui.add(
                    egui::ProgressBar::new(f32::from(remaining) / 100.0)
                        .text(format!("Estimated NVMe life remaining: {remaining}%")),
                );
            }
        });

        ui.add_space(18.0);
        section_heading(ui, "DEVICE DETAILS");
        egui::Frame::group(ui.style()).show(ui, |ui| {
            Grid::new("device_details")
                .num_columns(2)
                .spacing(Vec2::new(28.0, 8.0))
                .striped(true)
                .show(ui, |ui| {
                    detail_row(ui, "Device", &format!("/dev/{}", drive.name));
                    detail_row(ui, "Model", &drive.model);
                    detail_row(ui, "Serial number", available(&drive.serial));
                    detail_row(ui, "Drive type", &drive.kind);
                    detail_row(ui, "Capacity", &storage::format_capacity(drive.capacity));
                    detail_row(
                        ui,
                        "Temperature",
                        &drive
                            .temperature_c
                            .map(|value| format!("{value} °C"))
                            .unwrap_or_else(|| "Unavailable".into()),
                    );
                    detail_row(
                        ui,
                        "Power-on hours",
                        &drive
                            .power_on_hours
                            .map(|value| format!("{value} hours"))
                            .unwrap_or_else(|| "Unavailable".into()),
                    );
                    ui.end_row();
                });
        });

        if !drive.nvme_metrics.is_empty() {
            ui.add_space(18.0);
            section_heading(ui, "NVME HEALTH LOG");
            egui::Frame::group(ui.style()).show(ui, |ui| {
                Grid::new("nvme_health_log")
                    .num_columns(2)
                    .spacing(Vec2::new(28.0, 8.0))
                    .striped(true)
                    .show(ui, |ui| {
                        ui.label(RichText::new("Metric").strong());
                        ui.label(RichText::new("Value").strong());
                        ui.end_row();
                        for metric in &drive.nvme_metrics {
                            detail_row(ui, &metric.name, &metric.value);
                        }
                    });
            });
        }

        if !drive.ata_attributes.is_empty() {
            ui.add_space(18.0);
            section_heading(ui, "ATA S.M.A.R.T. ATTRIBUTES");
            egui::Frame::group(ui.style()).show(ui, |ui| {
                ScrollArea::horizontal().show(ui, |ui| {
                    Grid::new("ata_smart_attributes")
                        .num_columns(6)
                        .spacing(Vec2::new(18.0, 7.0))
                        .striped(true)
                        .show(ui, |ui| {
                            for heading in [
                                "ID",
                                "Attribute",
                                "Current",
                                "Worst",
                                "Threshold",
                                "Raw value",
                            ] {
                                ui.label(RichText::new(heading).strong());
                            }
                            ui.end_row();
                            for attribute in &drive.ata_attributes {
                                ui.label(attribute.id.to_string());
                                let name = if attribute.failed {
                                    format!("{} (failed)", attribute.name)
                                } else {
                                    attribute.name.clone()
                                };
                                ui.label(name);
                                ui.label(attribute.current.to_string());
                                ui.label(attribute.worst.to_string());
                                ui.label(attribute.threshold.to_string());
                                ui.label(&attribute.raw);
                                ui.end_row();
                            }
                        });
                });
            });
        }
    });
}

fn section_heading(ui: &mut egui::Ui, title: &str) {
    ui.label(RichText::new(title).strong().weak());
    ui.add_space(8.0);
}

fn detail_row(ui: &mut egui::Ui, name: &str, value: &str) {
    ui.label(RichText::new(name).weak());
    ui.label(value);
    ui.end_row();
}

fn available(value: &str) -> &str {
    if value.is_empty() {
        "Unavailable"
    } else {
        value
    }
}
