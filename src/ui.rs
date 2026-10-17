use crate::storage::{self, Drive};
use eframe::egui::{
    self, Align, Align2, Color32, FontId, Grid, Layout, Pos2, RichText, ScrollArea, Stroke,
    StrokeKind, Vec2,
};
use serde::{Deserialize, Serialize};
use std::io::Write as IoWrite;
use std::{
    collections::HashMap, fmt::Write as _, process::Command, sync::mpsc, thread, time::Duration,
};

pub const MAIN_WINDOW_WIDTH: f32 = 1014.0;
pub const MAIN_WINDOW_HEIGHT: f32 = 694.0;
const MAIN_WINDOW_HIDDEN_SMART_HEIGHT: f32 = 420.0;

pub fn initial_window_size() -> [f32; 2] {
    let settings = load_settings();
    [
        MAIN_WINDOW_WIDTH,
        if settings.hide_smart_info {
            MAIN_WINDOW_HIDDEN_SMART_HEIGHT
        } else {
            MAIN_WINDOW_HEIGHT
        },
    ]
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum RawFormat {
    #[default]
    Hex,
    Decimal,
    TwoByte,
    OneByte,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DriveViewAction {
    PreviousDrive,
    NextDrive,
    HealthSetting,
    TemperatureSetting,
}

pub struct DiskInfoApp {
    drives: Vec<Drive>,
    selected: Option<String>,
    scan_receiver: Option<mpsc::Receiver<Vec<Drive>>>,
    loading: bool,
    scan_error: Option<String>,
    logo_texture: egui::TextureHandle,
    fahrenheit: bool,
    hide_serial: bool,
    dark_theme: bool,
    green_mode: bool,
    raw_format: RawFormat,
    auto_refresh_seconds: Option<f64>,
    last_scan_time: f64,
    last_detection_time: f64,
    device_signature: storage::DeviceSignature,
    temperature_alert_times: HashMap<String, f64>,
    graph_open: bool,
    temperature_history: HashMap<String, Vec<(f64, f32)>>,
    settings: UserSettings,
    about_dialog: bool,
    temperature_dialog: bool,
    health_dialog: bool,
    sound_dialog: bool,
    settings_drive: String,
    temperature_draft: i32,
    health_draft: HealthThresholds,
    save_text_dialog: bool,
    save_text_path: String,
    save_text_error: Option<String>,
    save_image_dialog: bool,
    save_image_path: String,
    save_image_error: Option<String>,
    save_image_overwrite: bool,
    save_image_receiver: Option<mpsc::Receiver<Result<(), String>>>,
    save_image_pending: bool,
    save_image_capture_requested: bool,
    startup_error: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct UserSettings {
    drives: HashMap<String, DriveSettings>,
    #[serde(default)]
    hide_serial: bool,
    #[serde(default)]
    fahrenheit: bool,
    #[serde(default = "default_dark_theme")]
    dark_theme: bool,
    #[serde(default)]
    green_mode: bool,
    #[serde(default)]
    dump_identify: bool,
    #[serde(default)]
    dump_smart_data: bool,
    #[serde(default)]
    dump_smart_threshold: bool,
    #[serde(default)]
    ascii_view: bool,
    #[serde(default)]
    hide_smart_info: bool,
    #[serde(default = "default_hide_no_smart_disk")]
    hide_no_smart_disk: bool,
    #[serde(default = "default_hide_raid_volume")]
    hide_raid_volume: bool,
    #[serde(default = "default_auto_refresh_minutes")]
    auto_refresh_minutes: u32,
    #[serde(default)]
    raw_values: u8,
    #[serde(default)]
    auto_detection_seconds: u32,
    #[serde(default)]
    event_log: bool,
    #[serde(default)]
    startup_enabled: bool,
    #[serde(default = "default_alert_sound")]
    alert_sound: bool,
    #[serde(default)]
    alert_sound_path: String,
    #[serde(default = "default_alert_sound_volume")]
    alert_sound_volume: u8,
}

impl Default for UserSettings {
    fn default() -> Self {
        Self {
            drives: HashMap::new(),
            hide_serial: false,
            fahrenheit: false,
            dark_theme: true,
            green_mode: false,
            dump_identify: false,
            dump_smart_data: false,
            dump_smart_threshold: false,
            ascii_view: false,
            hide_smart_info: false,
            hide_no_smart_disk: true,
            hide_raid_volume: default_hide_raid_volume(),
            auto_refresh_minutes: default_auto_refresh_minutes(),
            raw_values: 0,
            auto_detection_seconds: 0,
            event_log: false,
            startup_enabled: false,
            alert_sound: true,
            alert_sound_path: String::new(),
            alert_sound_volume: default_alert_sound_volume(),
        }
    }
}

fn default_dark_theme() -> bool {
    true
}

fn load_logo(context: &egui::Context) -> egui::TextureHandle {
    let bytes = include_bytes!("../assets/logo.png");
    let image = (|| {
        let mut reader = png::Decoder::new(std::io::Cursor::new(bytes))
            .read_info()
            .ok()?;
        let mut buffer = vec![0; reader.output_buffer_size()?];
        let info = reader.next_frame(&mut buffer).ok()?;
        (info.color_type == png::ColorType::Rgba && info.bit_depth == png::BitDepth::Eight).then(
            || {
                egui::ColorImage::from_rgba_unmultiplied(
                    [info.width as usize, info.height as usize],
                    &buffer[..info.buffer_size()],
                )
            },
        )
    })()
    .unwrap_or_else(|| egui::ColorImage::filled([1, 1], Color32::TRANSPARENT));
    context.load_texture("application_logo", image, egui::TextureOptions::LINEAR)
}

fn default_hide_no_smart_disk() -> bool {
    true
}

fn default_hide_raid_volume() -> bool {
    true
}

fn default_alarm_health_status() -> bool {
    true
}

fn default_alert_sound() -> bool {
    true
}

fn default_alert_sound_volume() -> u8 {
    80
}

fn play_alert_sound(custom_path: &str, volume: u8, urgency: &str) {
    if volume == 0 {
        return;
    }
    let volume = volume.min(100);
    let warning = urgency == "critical";
    let sound_id = if warning {
        "dialog-warning"
    } else {
        "dialog-information"
    };
    let default_path = format!("/usr/share/sounds/freedesktop/stereo/{sound_id}.oga");
    let sound_path =
        if !custom_path.trim().is_empty() && std::path::Path::new(custom_path).is_file() {
            custom_path
        } else {
            &default_path
        };
    let pulse_volume = u32::from(volume) * 65_536 / 100;
    if let Ok(mut child) = Command::new("paplay")
        .arg(format!("--volume={pulse_volume}"))
        .arg(sound_path)
        .spawn()
    {
        thread::spawn(move || {
            let success = child.wait().is_ok_and(|status| status.success());
            if !success {
                let _ = Command::new("canberra-gtk-play")
                    .arg(format!("--id={sound_id}"))
                    .arg(format!(
                        "--volume={:.1}",
                        20.0 * f64::from(volume).log10() - 40.0
                    ))
                    .status();
            }
        });
    } else {
        if let Ok(mut child) = Command::new("canberra-gtk-play")
            .arg(format!("--id={sound_id}"))
            .arg(format!(
                "--volume={:.1}",
                20.0 * f64::from(volume).log10() - 40.0
            ))
            .spawn()
        {
            thread::spawn(move || {
                let _ = child.wait();
            });
        }
    }
}

fn event_log_path() -> std::path::PathBuf {
    let state_dir = std::env::var_os("XDG_STATE_HOME")
        .map(std::path::PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME").map(|home| std::path::PathBuf::from(home).join(".local/state"))
        })
        .unwrap_or_else(|| std::path::PathBuf::from("."));
    state_dir.join("viperdiskinfo/events.jsonl")
}

fn append_event_log(drive: &Drive, messages: &[String], urgency: &str) {
    let path = event_log_path();
    let Some(parent) = path.parent() else {
        return;
    };
    if std::fs::create_dir_all(parent).is_err() {
        return;
    }
    let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    else {
        return;
    };
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or_default();
    let record = serde_json::json!({
        "timestamp_unix": timestamp,
        "device": drive.name,
        "model": drive.model,
        "urgency": urgency,
        "events": messages,
    });
    if let Ok(line) = serde_json::to_string(&record) {
        let _ = writeln!(file, "{line}");
    }
}

fn default_auto_refresh_minutes() -> u32 {
    10
}

fn raw_format_from_setting(value: u8) -> RawFormat {
    match value {
        1 => RawFormat::Decimal,
        2 => RawFormat::TwoByte,
        3 => RawFormat::OneByte,
        _ => RawFormat::Hex,
    }
}

fn raw_format_setting(format: RawFormat) -> u8 {
    match format {
        RawFormat::Hex => 0,
        RawFormat::Decimal => 1,
        RawFormat::TwoByte => 2,
        RawFormat::OneByte => 3,
    }
}

fn auto_refresh_interval(minutes: u32) -> Option<f64> {
    match minutes {
        1 | 3 | 5 | 10 | 30 | 60 | 120 | 180 | 360 | 720 | 1440 => Some(f64::from(minutes) * 60.0),
        _ => None,
    }
}

fn auto_detection_interval(seconds: u32) -> Option<f64> {
    match seconds {
        5 | 10 | 20 | 30 => Some(f64::from(seconds)),
        _ => None,
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct DriveSettings {
    temperature_alarm: i32,
    #[serde(default = "default_alarm_health_status")]
    alarm_health_status: bool,
    threshold_05: i32,
    threshold_c5: i32,
    threshold_c6: i32,
    threshold_ff: i32,
}

#[derive(Clone, Debug)]
struct HealthThresholds {
    alarm_health_status: bool,
    threshold_05: i32,
    threshold_c5: i32,
    threshold_c6: i32,
    threshold_ff: i32,
}

impl Default for HealthThresholds {
    fn default() -> Self {
        Self {
            alarm_health_status: true,
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
            alarm_health_status: true,
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
    base.join("viperdiskinfo").join("settings.json")
}

fn autostart_path() -> Result<std::path::PathBuf, String> {
    let config_dir = std::env::var_os("XDG_CONFIG_HOME")
        .map(std::path::PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME").map(|home| std::path::PathBuf::from(home).join(".config"))
        })
        .ok_or_else(|| "Could not determine the user's configuration directory.".to_string())?;
    Ok(config_dir.join("autostart/viperdiskinfo.desktop"))
}

const AUTOSTART_MARKER: &str = "X-ViperDiskInfo-Autostart=true";

fn autostart_enabled() -> bool {
    autostart_path()
        .ok()
        .and_then(|path| std::fs::read_to_string(path).ok())
        .is_some_and(|entry| entry.lines().any(|line| line == AUTOSTART_MARKER))
}

fn set_autostart_enabled(enabled: bool) -> Result<(), String> {
    let path = autostart_path()?;
    if enabled {
        if path.exists() {
            let content = std::fs::read_to_string(&path)
                .map_err(|error| format!("Could not read existing autostart entry: {error}"))?;
            if !content.lines().any(|line| line == AUTOSTART_MARKER) {
                return Err(format!(
                    "{} already exists and is not managed by Viper Disk Info.",
                    path.display()
                ));
            }
        }
        let executable = if let Some(appimage) = std::env::var_os("APPIMAGE") {
            std::path::PathBuf::from(appimage)
        } else {
            std::env::current_exe()
                .map_err(|error| format!("Could not locate the application: {error}"))?
        };
        let exec = desktop_exec_quoted(&executable);
        let entry = format!(
            "[Desktop Entry]\nType=Application\nName=Viper Disk Info\nComment=Monitor disk health\nExec={exec}\nTerminal=false\nX-GNOME-Autostart-enabled=true\n{AUTOSTART_MARKER}\n"
        );
        let parent = path
            .parent()
            .ok_or_else(|| "Could not determine the autostart directory.".to_string())?;
        std::fs::create_dir_all(parent)
            .map_err(|error| format!("Could not create autostart directory: {error}"))?;
        let temporary = path.with_extension(format!("desktop.{}.tmp", std::process::id()));
        if temporary.exists() {
            return Err(format!(
                "Temporary autostart file already exists: {}",
                temporary.display()
            ));
        }
        if let Err(error) = std::fs::write(&temporary, entry) {
            let _ = std::fs::remove_file(&temporary);
            return Err(format!("Could not write autostart entry: {error}"));
        }
        if let Err(error) = std::fs::rename(&temporary, &path) {
            let _ = std::fs::remove_file(&temporary);
            return Err(format!("Could not install autostart entry: {error}"));
        }
    } else if path.exists() {
        let content = std::fs::read_to_string(&path)
            .map_err(|error| format!("Could not read autostart entry: {error}"))?;
        if !content.lines().any(|line| line == AUTOSTART_MARKER) {
            return Err(format!(
                "{} is not managed by Viper Disk Info, so it was left in place.",
                path.display()
            ));
        }
        std::fs::remove_file(&path)
            .map_err(|error| format!("Could not remove autostart entry: {error}"))?;
    }
    Ok(())
}

fn desktop_exec_quoted(path: &std::path::Path) -> String {
    let mut quoted = String::from("\"");
    for character in path.to_string_lossy().chars() {
        if character == '%' {
            quoted.push_str("%%");
            continue;
        }
        if matches!(character, '\\' | '"' | '$' | '`') {
            quoted.push('\\');
        }
        quoted.push(character);
    }
    quoted.push('"');
    quoted
}

fn legacy_settings_path() -> std::path::PathBuf {
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
        .or_else(|_| std::fs::read(legacy_settings_path()))
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
        let mut settings = load_settings();
        settings.startup_enabled = autostart_enabled();
        let raw_format = raw_format_from_setting(settings.raw_values);
        let auto_refresh_seconds = auto_refresh_interval(settings.auto_refresh_minutes);
        let logo_texture = load_logo(&context.egui_ctx);
        let mut app = Self {
            drives: Vec::new(),
            selected: None,
            scan_receiver: None,
            loading: false,
            scan_error: None,
            logo_texture,
            fahrenheit: false,
            hide_serial: false,
            dark_theme: true,
            green_mode: false,
            raw_format,
            auto_refresh_seconds,
            last_scan_time: 0.0,
            last_detection_time: 0.0,
            device_signature: Vec::new(),
            temperature_alert_times: HashMap::new(),
            graph_open: false,
            temperature_history: HashMap::new(),
            settings,
            about_dialog: false,
            temperature_dialog: false,
            health_dialog: false,
            sound_dialog: false,
            settings_drive: String::new(),
            temperature_draft: 50,
            health_draft: HealthThresholds::default(),
            save_text_dialog: false,
            save_text_path: String::new(),
            save_text_error: None,
            save_image_dialog: false,
            save_image_path: String::new(),
            save_image_error: None,
            save_image_overwrite: false,
            save_image_receiver: None,
            save_image_pending: false,
            save_image_capture_requested: false,
            startup_error: None,
        };
        app.hide_serial = app.settings.hide_serial;
        app.fahrenheit = app.settings.fahrenheit;
        app.dark_theme = app.settings.dark_theme;
        app.green_mode = app.settings.green_mode;
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
                self.send_drive_alerts(&drives, now);
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
                self.drives = drives;
                self.device_signature = storage::device_signature();
                let visible = self.visible_drives();
                if self
                    .selected
                    .as_ref()
                    .is_none_or(|selected| !visible.iter().any(|drive| &drive.name == selected))
                {
                    self.selected = visible.first().map(|drive| drive.name.clone());
                }
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
        self.drives.iter().find(|drive| {
            Some(drive.name.as_str()) == self.selected.as_deref() && self.drive_visible(drive)
        })
    }

    fn visible_drives(&self) -> Vec<Drive> {
        self.drives
            .iter()
            .filter(|drive| self.drive_visible(drive))
            .cloned()
            .collect()
    }

    fn drive_visible(&self, drive: &Drive) -> bool {
        (!self.settings.hide_no_smart_disk || drive.smart_available)
            && (!self.settings.hide_raid_volume
                || !drive.model.to_ascii_uppercase().contains("RAID"))
    }

    fn send_drive_alerts(&mut self, current_drives: &[Drive], now: f64) {
        for drive in current_drives {
            let settings = self
                .settings
                .drives
                .get(&drive.name)
                .cloned()
                .unwrap_or_else(|| DriveSettings::defaults(drive));
            let previous = self.drives.iter().find(|old| old.name == drive.name);
            let mut messages = Vec::new();
            let mut urgency = "normal";

            if settings.alarm_health_status
                && let Some(previous) = previous
            {
                let previous_status =
                    status_label_with_settings(previous, &settings, self.green_mode).0;
                let current_status =
                    status_label_with_settings(drive, &settings, self.green_mode).0;
                if previous_status != "Unknown"
                    && current_status != "Unknown"
                    && previous_status != current_status
                {
                    messages.push(format!(
                        "Health status: {previous_status} -> {current_status}"
                    ));
                    if status_severity(current_status) > status_severity(previous_status) {
                        urgency = "critical";
                    }
                }

                if let (Some(old), Some(new)) = (previous.health_percent, drive.health_percent)
                    && new != old
                {
                    messages.push(format!("Estimated life: {old}% -> {new}%"));
                    if new < old {
                        urgency = "critical";
                    }
                }

                let is_ssd = drive
                    .rotation_rate
                    .as_deref()
                    .is_some_and(|rate| rate == "SSD" || rate == "0");
                if !is_ssd {
                    for id in [0x05, 0xC5, 0xC6] {
                        let old = previous
                            .ata_attributes
                            .iter()
                            .find(|attribute| attribute.id == id)
                            .and_then(|attribute| attribute.raw_value)
                            .map(|value| value as u16);
                        let new = drive
                            .ata_attributes
                            .iter()
                            .find(|attribute| attribute.id == id)
                            .and_then(|attribute| attribute.raw_value)
                            .map(|value| value as u16);
                        if let (Some(old), Some(new)) = (old, new)
                            && new != old
                        {
                            let name = drive
                                .ata_attributes
                                .iter()
                                .find(|attribute| attribute.id == id)
                                .map(|attribute| attribute.name.as_str())
                                .unwrap_or("SMART attribute");
                            messages.push(format!("{name}: {old} -> {new}"));
                            if new > old {
                                urgency = "critical";
                            }
                        }
                    }
                }
            }

            if settings.temperature_alarm > 0
                && drive
                    .temperature_c
                    .is_some_and(|temperature| temperature >= i64::from(settings.temperature_alarm))
            {
                let last = self.temperature_alert_times.get(&drive.name).copied();
                if last.is_none_or(|last| now - last >= 3600.0) {
                    let temperature = drive.temperature_c.unwrap_or_default();
                    messages.push(format!(
                        "Temperature alert: {}",
                        show_temperature(temperature, self.fahrenheit)
                    ));
                    self.temperature_alert_times.insert(drive.name.clone(), now);
                    urgency = "critical";
                }
            }

            if !messages.is_empty() {
                let title = format!("{} health alert", drive.model);
                let body = messages.join("\n");
                if self.settings.event_log {
                    append_event_log(drive, &messages, urgency);
                }
                if self.settings.alert_sound {
                    play_alert_sound(
                        &self.settings.alert_sound_path,
                        self.settings.alert_sound_volume,
                        urgency,
                    );
                }
                if let Ok(mut child) = Command::new("notify-send")
                    .arg("--app-name=Viper Disk Info")
                    .arg(format!("--urgency={urgency}"))
                    .arg("--expire-time=10000")
                    .arg("--hint=boolean:suppress-sound:true")
                    .arg(title)
                    .arg(body)
                    .spawn()
                {
                    thread::spawn(move || {
                        let _ = child.wait();
                    });
                }
            }
        }
    }

    fn set_interval(&mut self, seconds: Option<f64>) {
        self.auto_refresh_seconds = seconds;
        self.settings.auto_refresh_minutes = seconds
            .map(|seconds| (seconds / 60.0).round() as u32)
            .unwrap_or_default();
        save_settings(&self.settings);
        self.last_scan_time = 0.0;
    }
}

impl eframe::App for DiskInfoApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let context = ui.ctx().clone();
        self.poll_scan(&context);
        self.poll_image_save();
        if self.save_image_capture_requested {
            self.save_image_capture_requested = false;
            let path = std::path::PathBuf::from(self.save_image_path.clone());
            let (sender, receiver) = mpsc::channel();
            self.save_image_receiver = Some(receiver);
            let repaint = context.clone();
            context.request_screenshot(move |image| {
                let result = write_screenshot(&path, &image);
                let _ = sender.send(result);
                repaint.request_repaint();
            });
        }
        if let Some(interval) = self.auto_refresh_seconds
            && !self.loading
            && context.input(|input| input.time) - self.last_scan_time >= interval
        {
            self.start_scan(context.clone());
        }
        let now = context.input(|input| input.time);
        if let Some(interval) = auto_detection_interval(self.settings.auto_detection_seconds)
            && now - self.last_detection_time >= interval
        {
            self.last_detection_time = now;
            if !self.loading && storage::device_signature() != self.device_signature {
                self.start_scan(context.clone());
            }
        }
        if context.input(|input| input.key_pressed(egui::Key::F5)) {
            self.start_scan(context.clone());
        }
        if context.input(|input| input.key_pressed(egui::Key::F6)) {
            self.start_scan(context.clone());
        }
        if context.input(|input| {
            input.key_pressed(egui::Key::C) && input.modifiers.ctrl && input.modifiers.shift
        }) {
            context.copy_text(self.report_text());
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
                if let Some(drive) = self.current_drive().cloned() {
                    let settings = self
                        .settings
                        .drives
                        .get(&drive.name)
                        .cloned()
                        .unwrap_or_else(|| DriveSettings::defaults(&drive));
                    let clicked_setting = drive_view(
                        ui,
                        &drive,
                        &settings,
                        self.fahrenheit,
                        self.hide_serial,
                        self.green_mode,
                        self.raw_format,
                        self.settings.hide_smart_info,
                    );
                    match clicked_setting {
                        Some(DriveViewAction::PreviousDrive) => {
                            let visible = self.visible_drives();
                            if !visible.is_empty() {
                                let index = visible
                                    .iter()
                                    .position(|item| item.name == drive.name)
                                    .unwrap_or_default();
                                let previous = (index + visible.len() - 1) % visible.len();
                                self.selected = Some(visible[previous].name.clone());
                            }
                        }
                        Some(DriveViewAction::NextDrive) => {
                            let visible = self.visible_drives();
                            if !visible.is_empty() {
                                let index = visible
                                    .iter()
                                    .position(|item| item.name == drive.name)
                                    .unwrap_or_default();
                                self.selected =
                                    Some(visible[(index + 1) % visible.len()].name.clone());
                            }
                        }
                        Some(DriveViewAction::HealthSetting) => {
                            self.begin_settings();
                            self.health_dialog = true;
                        }
                        Some(DriveViewAction::TemperatureSetting) => {
                            self.begin_settings();
                            self.temperature_dialog = true;
                        }
                        _ => {}
                    }
                } else if self.loading {
                    ui.centered_and_justified(|ui| {
                        ui.spinner();
                        ui.label("Scanning drives...");
                    });
                } else if let Some(error) = &self.scan_error {
                    ui.centered_and_justified(|ui| {
                        ui.colored_label(Color32::LIGHT_RED, error);
                    });
                } else if !self.drives.is_empty() {
                    ui.centered_and_justified(|ui| {
                        ui.label("No disks match the current filters.");
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
        self.sound_settings_window(&context);
        self.about_window(&context);
        self.save_text_window(&context);
        self.save_image_window(&context);

        let refresh_wait = self
            .auto_refresh_seconds
            .filter(|_| !self.loading)
            .map(|interval| interval - (now - self.last_scan_time));
        let detection_wait = auto_detection_interval(self.settings.auto_detection_seconds)
            .map(|interval| interval - (now - self.last_detection_time));
        if let Some(wait) = [refresh_wait, detection_wait]
            .into_iter()
            .flatten()
            .min_by(f64::total_cmp)
        {
            context.request_repaint_after(Duration::from_secs_f64(wait.max(0.05)));
        }
    }
}

impl DiskInfoApp {
    fn menu_bar(&mut self, ui: &mut egui::Ui, context: &egui::Context) {
        egui::MenuBar::new().ui(ui, |ui| {
            ui.menu_button("File", |ui| {
                if ui.button("Save text...").clicked() {
                    self.save_text_path.clear();
                    self.save_text_error = None;
                    self.save_text_dialog = true;
                    ui.close();
                }
                if ui.button("Save image...").clicked() {
                    self.save_image_path.clear();
                    self.save_image_error = None;
                    self.save_image_overwrite = false;
                    self.save_image_dialog = true;
                    ui.close();
                }
                ui.separator();
                if ui.button("Exit").clicked() {
                    context.send_viewport_cmd(egui::ViewportCommand::Close);
                    ui.close();
                }
            });
            ui.menu_button("Edit", |ui| {
                if ui.button("Copy\tCtrl + Shift + C").clicked() {
                    context.copy_text(self.report_text());
                    ui.close();
                }
                ui.menu_button("Options", |ui| {
                    let mut changed = false;
                    changed |= ui
                        .checkbox(&mut self.settings.dump_identify, "IDENTIFY_DEVICE")
                        .changed();
                    changed |= ui
                        .checkbox(&mut self.settings.dump_smart_data, "SMART_READ_DATA")
                        .changed();
                    changed |= ui
                        .checkbox(
                            &mut self.settings.dump_smart_threshold,
                            "SMART_READ_THRESHOLD",
                        )
                        .changed();
                    ui.separator();
                    changed |= ui
                        .checkbox(&mut self.settings.ascii_view, "ASCII View")
                        .changed();
                    if changed {
                        save_settings(&self.settings);
                    }
                });
                if ui
                    .checkbox(&mut self.hide_serial, "Hide serial number")
                    .changed()
                {
                    self.settings.hide_serial = self.hide_serial;
                    save_settings(&self.settings);
                }
            });
            ui.menu_button("Function", |ui| {
                if ui.button("Refresh     F5").clicked() {
                    self.start_scan(context.clone());
                    ui.close();
                }
                if ui
                    .checkbox(&mut self.settings.startup_enabled, "Startup")
                    .changed()
                {
                    let enable = self.settings.startup_enabled;
                    match set_autostart_enabled(enable) {
                        Ok(()) => {
                            self.startup_error = None;
                            save_settings(&self.settings);
                        }
                        Err(error) => {
                            self.settings.startup_enabled = !enable;
                            self.startup_error = Some(error);
                        }
                    }
                }
                if let Some(error) = &self.startup_error {
                    ui.colored_label(Color32::LIGHT_RED, error);
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
                ui.menu_button("Auto Detection", |ui| {
                    for (label, seconds) in [
                        ("Disable", 0),
                        ("5 sec.", 5),
                        ("10 sec.", 10),
                        ("20 sec.", 20),
                        ("30 sec.", 30),
                    ] {
                        let selected = self.settings.auto_detection_seconds == seconds;
                        if ui.selectable_label(selected, label).clicked() {
                            self.settings.auto_detection_seconds = seconds;
                            self.last_detection_time = context.input(|input| input.time);
                            save_settings(&self.settings);
                            ui.close();
                        }
                    }
                });
                ui.menu_button("Alert Features", |ui| {
                    if ui
                        .checkbox(&mut self.settings.event_log, "Event Log")
                        .changed()
                    {
                        save_settings(&self.settings);
                    }
                    if ui
                        .checkbox(&mut self.settings.alert_sound, "Alert Sound")
                        .changed()
                    {
                        save_settings(&self.settings);
                    }
                    if ui.button("Sound Settings...").clicked() {
                        self.sound_dialog = true;
                        ui.close();
                    }
                });
                ui.separator();
                if ui.button("Graph").clicked() {
                    self.graph_open = true;
                    ui.close();
                }
                ui.menu_button("Advanced Features", |ui| {
                    if ui.button("Health Status Setting").clicked() {
                        self.begin_settings();
                        self.health_dialog = true;
                        ui.close();
                    }
                    if ui.button("Temperature Setting").clicked() {
                        self.begin_settings();
                        self.temperature_dialog = true;
                        ui.close();
                    }
                    ui.menu_button("Temperature", |ui| {
                        if ui
                            .selectable_value(&mut self.fahrenheit, false, "Celsius")
                            .changed()
                        {
                            self.settings.fahrenheit = self.fahrenheit;
                            save_settings(&self.settings);
                        }
                        if ui
                            .selectable_value(&mut self.fahrenheit, true, "Fahrenheit")
                            .changed()
                        {
                            self.settings.fahrenheit = self.fahrenheit;
                            save_settings(&self.settings);
                        }
                    });
                    if ui.checkbox(&mut self.green_mode, "Green mode").changed() {
                        self.settings.green_mode = self.green_mode;
                        save_settings(&self.settings);
                    }
                });
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
                            self.settings.raw_values = raw_format_setting(format);
                            save_settings(&self.settings);
                            ui.close();
                        }
                    }
                });
            });
            ui.menu_button("Theme", |ui| {
                if ui.selectable_label(self.dark_theme, "Dark").clicked() {
                    self.dark_theme = true;
                    self.settings.dark_theme = true;
                    save_settings(&self.settings);
                    ui.close();
                }
                if ui.selectable_label(!self.dark_theme, "Light").clicked() {
                    self.dark_theme = false;
                    self.settings.dark_theme = false;
                    save_settings(&self.settings);
                    ui.close();
                }
            });
            ui.menu_button("Disk", |ui| {
                let visible = self.visible_drives();
                for drive in &visible {
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
                ui.separator();
                if ui
                    .checkbox(
                        &mut self.settings.hide_smart_info,
                        "Hide S.M.A.R.T. Information",
                    )
                    .changed()
                {
                    save_settings(&self.settings);
                    context.send_viewport_cmd(egui::ViewportCommand::InnerSize(Vec2::new(
                        MAIN_WINDOW_WIDTH,
                        if self.settings.hide_smart_info {
                            MAIN_WINDOW_HIDDEN_SMART_HEIGHT
                        } else {
                            MAIN_WINDOW_HEIGHT
                        },
                    )));
                }
                if ui
                    .checkbox(
                        &mut self.settings.hide_no_smart_disk,
                        "Hide No S.M.A.R.T. Disk",
                    )
                    .changed()
                {
                    save_settings(&self.settings);
                    let visible = self.visible_drives();
                    if self
                        .selected
                        .as_ref()
                        .is_none_or(|selected| !visible.iter().any(|drive| &drive.name == selected))
                    {
                        self.selected = visible.first().map(|drive| drive.name.clone());
                    }
                }
                if ui
                    .checkbox(&mut self.settings.hide_raid_volume, "Hide RAID Volume")
                    .changed()
                {
                    save_settings(&self.settings);
                    let visible = self.visible_drives();
                    if self
                        .selected
                        .as_ref()
                        .is_none_or(|selected| !visible.iter().any(|drive| &drive.name == selected))
                    {
                        self.selected = visible.first().map(|drive| drive.name.clone());
                    }
                }
            });
            ui.menu_button("Help", |ui| {
                ui.hyperlink_to("Help [Web]", "https://crystalmark.info/");
                ui.hyperlink_to(
                    "About S.M.A.R.T. (Wikipedia) [Web]",
                    "https://en.wikipedia.org/wiki/Self-Monitoring,_Analysis_and_Reporting_Technology",
                );
                ui.separator();
                if ui.button("About Viper Disk Info").clicked() {
                    self.about_dialog = true;
                    ui.close();
                }
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
            alarm_health_status: settings.alarm_health_status,
            threshold_05: settings.threshold_05,
            threshold_c5: settings.threshold_c5,
            threshold_c6: settings.threshold_c6,
            threshold_ff: settings.threshold_ff,
        };
    }

    fn settings_windows(&mut self, context: &egui::Context) {
        let visible_drives = self.visible_drives();
        let drive_index = visible_drives
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
                                visible_drives[index].model, visible_drives[index].name
                            ))
                            .show_ui(ui, |ui| {
                                for drive in &visible_drives {
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
                                egui::Slider::new(&mut self.temperature_draft, 0..=90)
                                    .suffix(" °C"),
                            );
                        });
                        if self.temperature_draft == 0 {
                            ui.label("Temperature alerts disabled");
                        } else {
                            ui.label(format!(
                                "{} °C / {} °F",
                                self.temperature_draft,
                                self.temperature_draft * 9 / 5 + 32
                            ));
                        }
                        ui.horizontal(|ui| {
                            if ui.button("Default").clicked() {
                                self.temperature_draft =
                                    DriveSettings::defaults(&visible_drives[index])
                                        .temperature_alarm;
                            }
                            if ui.button("Apply").clicked() {
                                let drive = &visible_drives[index];
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
                                visible_drives[index].model, visible_drives[index].name
                            ))
                            .show_ui(ui, |ui| {
                                for drive in &visible_drives {
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
                                            alarm_health_status: saved.alarm_health_status,
                                            threshold_05: saved.threshold_05,
                                            threshold_c5: saved.threshold_c5,
                                            threshold_c6: saved.threshold_c6,
                                            threshold_ff: saved.threshold_ff,
                                        };
                                    }
                                }
                            });
                        ui.checkbox(
                            &mut self.health_draft.alarm_health_status,
                            "Alert when health status worsens",
                        );
                        let ssd = visible_drives[index]
                            .rotation_rate
                            .as_deref()
                            .is_some_and(|rate| rate == "SSD" || rate == "0");
                        if ssd {
                            if visible_drives[index].health_percent.is_some() {
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
                                let drive = &visible_drives[index];
                                let entry = self
                                    .settings
                                    .drives
                                    .entry(drive.name.clone())
                                    .or_insert_with(|| DriveSettings::defaults(drive));
                                entry.alarm_health_status = self.health_draft.alarm_health_status;
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

    fn sound_settings_window(&mut self, context: &egui::Context) {
        if !self.sound_dialog {
            return;
        }
        let mut open = self.sound_dialog;
        egui::Window::new("Sound Settings")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .show(context, |ui| {
                ui.label("Leave the sound file empty to use the desktop alert sounds.");
                ui.horizontal(|ui| {
                    ui.label("Sound file");
                    if ui
                        .text_edit_singleline(&mut self.settings.alert_sound_path)
                        .changed()
                    {
                        save_settings(&self.settings);
                    }
                });
                if ui
                    .add(
                        egui::Slider::new(&mut self.settings.alert_sound_volume, 0..=100)
                            .text("Volume"),
                    )
                    .changed()
                {
                    save_settings(&self.settings);
                }
                ui.horizontal(|ui| {
                    if ui.button("Test").clicked() {
                        play_alert_sound(
                            &self.settings.alert_sound_path,
                            self.settings.alert_sound_volume,
                            "critical",
                        );
                    }
                    if ui.button("Default").clicked() {
                        self.settings.alert_sound_path.clear();
                        self.settings.alert_sound_volume = default_alert_sound_volume();
                        save_settings(&self.settings);
                    }
                });
            });
        self.sound_dialog = open;
    }

    fn about_window(&mut self, context: &egui::Context) {
        if !self.about_dialog {
            return;
        }
        let mut open = self.about_dialog;
        let mut close_clicked = false;
        egui::Window::new("About Viper Disk Info")
            .open(&mut open)
            .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
            .default_width(390.0)
            .collapsible(false)
            .resizable(false)
            .show(context, |ui| {
                ui.vertical_centered(|ui| {
                    ui.image((self.logo_texture.id(), Vec2::splat(112.0)));
                    ui.heading("Viper Disk Info");
                    ui.label(format!("Version {}", env!("CARGO_PKG_VERSION")));
                    ui.label("Disk health and S.M.A.R.T. information for Linux");
                });
                ui.separator();
                ui.label("Interface and behavior reference");
                ui.hyperlink_to("CrystalDiskInfo 9.9.3", "https://crystalmark.info/");
                ui.add_space(6.0);
                ui.label("S.M.A.R.T. data provider");
                ui.hyperlink_to("smartmontools", "https://www.smartmontools.org/");
                ui.add_space(6.0);
                ui.label("License: MIT");
                ui.label("CrystalDiskInfo copyright: hiyohiyo and CrystalMark Inc.");
                ui.separator();
                ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("Close").clicked() {
                        close_clicked = true;
                    }
                });
            });
        if close_clicked {
            open = false;
        }
        self.about_dialog = open;
    }

    fn drive_tabs(&mut self, ui: &mut egui::Ui) {
        ScrollArea::horizontal()
            .auto_shrink([false, true])
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    let visible = self.visible_drives();
                    for (index, drive) in visible.iter().enumerate() {
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
                    if visible.is_empty() && !self.loading {
                        ui.label(if self.drives.is_empty() {
                            "No detected disks"
                        } else {
                            "No disks with S.M.A.R.T. data"
                        });
                    }
                });
            });
    }

    fn report_text(&self) -> String {
        let visible_drives = self.visible_drives();
        let mut report = String::from(
            "----------------------------------------------------------------------------\nViper Disk Info\n----------------------------------------------------------------------------\n\n-- Disk List ---------------------------------------------------------------\n",
        );
        for (index, drive) in visible_drives.iter().enumerate() {
            let _ = writeln!(
                report,
                " ({:02}) {} : {} [/dev/{}]",
                index + 1,
                drive.model,
                storage::format_capacity(drive.capacity),
                drive.name
            );
        }
        for (index, drive) in visible_drives.iter().enumerate() {
            let _ = writeln!(
                report,
                "\n----------------------------------------------------------------------------\n ({:02}) {}\n----------------------------------------------------------------------------",
                index + 1,
                drive.model
            );
            let _ = writeln!(report, "           Model : {}", drive.model);
            let _ = writeln!(report, "          Device : /dev/{}", drive.name);
            let _ = writeln!(report, "       Firmware : {}", option(&drive.firmware));
            let _ = writeln!(
                report,
                "   Serial Number : {}",
                if self.hide_serial {
                    "********"
                } else {
                    available(&drive.serial)
                }
            );
            let _ = writeln!(
                report,
                "       Disk Size : {}",
                storage::format_capacity(drive.capacity)
            );
            let _ = writeln!(report, "       Interface : {}", option(&drive.interface));
            let _ = writeln!(report, "        Standard : {}", drive.kind);
            let _ = writeln!(
                report,
                "   Transfer Mode : {}",
                option(&drive.transfer_mode)
            );
            let _ = writeln!(
                report,
                "  Power On Hours : {}",
                drive
                    .power_on_hours
                    .map(|v| format!("{v} hours"))
                    .unwrap_or_else(|| "Unknown".into())
            );
            let _ = writeln!(
                report,
                "   Power On Count : {}",
                drive
                    .power_cycles
                    .map(|v| v.to_string())
                    .unwrap_or_else(|| "Unknown".into())
            );
            let _ = writeln!(
                report,
                "Temperature: {}",
                drive
                    .temperature_c
                    .map(|value| show_temperature(value, self.fahrenheit))
                    .unwrap_or_else(|| "Unavailable".into())
            );
            let settings = self
                .settings
                .drives
                .get(&drive.name)
                .cloned()
                .unwrap_or_else(|| DriveSettings::defaults(drive));
            let (health_status, _) = status_label_with_settings(drive, &settings, self.green_mode);
            let _ = writeln!(report, "   Health Status : {health_status}");
            let _ = writeln!(report, "    Drive Letter : {}", drive.drive_map.join(", "));
            if !drive.ata_attributes.is_empty() {
                report.push_str("\n-- S.M.A.R.T. --------------------------------------------------------------\n");
            }
            for attribute in &drive.ata_attributes {
                let _ = writeln!(
                    report,
                    " {:02X} {} | Current: {} | Worst: {} | Threshold: {} | Raw: {}",
                    attribute.id,
                    attribute.name,
                    attribute.current,
                    attribute.worst,
                    attribute.threshold,
                    attribute.raw,
                );
            }
            if self.settings.dump_identify {
                report.push_str("\n-- IDENTIFY_DEVICE ---------------------------------------------------------\n");
                if self.hide_serial {
                    report.push_str(
                        "Raw IDENTIFY output omitted while serial number hiding is enabled.\n",
                    );
                } else {
                    report.push_str(&smartctl_output(
                        &drive.name,
                        &["-r", "ataioctl,2", "-i"],
                        self.settings.ascii_view,
                    ));
                }
            }
            if self.settings.dump_smart_data
                || (self.settings.dump_smart_threshold && !drive.kind.eq_ignore_ascii_case("NVMe"))
            {
                let trace = if drive.kind.eq_ignore_ascii_case("NVMe") {
                    "nvmeioctl,2"
                } else {
                    "ataioctl,2"
                };
                let dump =
                    smartctl_output(&drive.name, &["-r", trace, "-A"], self.settings.ascii_view);
                let data_enabled = self.settings.dump_smart_data;
                let threshold_enabled =
                    self.settings.dump_smart_threshold && !drive.kind.eq_ignore_ascii_case("NVMe");
                match (data_enabled, threshold_enabled) {
                    (true, true) => {
                        report.push_str("\n-- SMART_READ_DATA + SMART_READ_THRESHOLD -------------------------------\n");
                        report.push_str("smartctl returns both reads together for this device.\n");
                        report.push_str(&dump);
                    }
                    (true, false) => {
                        report.push_str("\n-- SMART_READ_DATA ---------------------------------------------------------\n");
                        report.push_str(&dump);
                    }
                    (false, true) => {
                        report.push_str("\n-- SMART_READ_THRESHOLD ----------------------------------------------------\n");
                        report.push_str(&dump);
                    }
                    (false, false) => {}
                }
            }
            if !drive.nvme_metrics.is_empty() {
                report.push_str("\n-- NVMe S.M.A.R.T. ---------------------------------------------------------\n");
                for metric in &drive.nvme_metrics {
                    let _ = writeln!(report, " {} : {}", metric.name, metric.value);
                }
            }
        }
        report
    }

    fn save_text_window(&mut self, context: &egui::Context) {
        if !self.save_text_dialog {
            return;
        }
        let mut open = self.save_text_dialog;
        egui::Window::new("Save text report")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .show(context, |ui| {
                ui.label("Save the full report for all detected drives.");
                ui.horizontal(|ui| {
                    ui.label("File path");
                    ui.text_edit_singleline(&mut self.save_text_path);
                });
                if let Some(error) = &self.save_text_error {
                    ui.colored_label(Color32::LIGHT_RED, error);
                }
                ui.horizontal(|ui| {
                    if ui.button("Save").clicked() {
                        let result = if self.save_text_path.trim().is_empty() {
                            Err("Enter a file path.".to_string())
                        } else {
                            std::fs::write(&self.save_text_path, self.report_text())
                                .map_err(|error| format!("Could not save report: {error}"))
                        };
                        match result {
                            Ok(()) => {
                                self.save_text_error = None;
                                self.save_text_dialog = false;
                            }
                            Err(error) => self.save_text_error = Some(error),
                        }
                    }
                    if ui.button("Cancel").clicked() {
                        self.save_text_dialog = false;
                    }
                });
            });
        self.save_text_dialog = open && self.save_text_dialog;
    }

    fn poll_image_save(&mut self) {
        let Some(receiver) = self.save_image_receiver.take() else {
            return;
        };
        match receiver.try_recv() {
            Ok(Ok(())) => {
                self.save_image_pending = false;
                self.save_image_dialog = false;
                self.save_image_error = None;
            }
            Ok(Err(error)) => {
                self.save_image_pending = false;
                self.save_image_dialog = true;
                self.save_image_error = Some(error);
            }
            Err(mpsc::TryRecvError::Empty) => {
                self.save_image_receiver = Some(receiver);
            }
            Err(mpsc::TryRecvError::Disconnected) => {
                self.save_image_pending = false;
                self.save_image_dialog = true;
                self.save_image_error = Some("Screenshot capture did not complete.".into());
            }
        }
    }

    fn save_image_window(&mut self, context: &egui::Context) {
        if !self.save_image_dialog {
            return;
        }
        let mut open = self.save_image_dialog;
        egui::Window::new("Save image")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .show(context, |ui| {
                ui.label("Save the current Viper Disk Info window as PNG or BMP.");
                let response = ui.text_edit_singleline(&mut self.save_image_path);
                if response.changed() {
                    self.save_image_overwrite = false;
                    self.save_image_error = None;
                }
                if self.save_image_pending {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label("Capturing image...");
                    });
                }
                if let Some(error) = &self.save_image_error {
                    ui.colored_label(Color32::LIGHT_RED, error);
                }
                ui.horizontal(|ui| {
                    let save = ui.add_enabled(!self.save_image_pending, egui::Button::new("Save"));
                    if save.clicked() {
                        let path = std::path::PathBuf::from(self.save_image_path.trim());
                        let supported =
                            path.extension()
                                .and_then(|ext| ext.to_str())
                                .is_some_and(|ext| {
                                    matches!(ext.to_ascii_lowercase().as_str(), "png" | "bmp")
                                });
                        if self.save_image_path.trim().is_empty() {
                            self.save_image_error =
                                Some("Enter a file path ending in .png or .bmp.".into());
                        } else if !supported {
                            self.save_image_error = Some("Choose a .png or .bmp file path.".into());
                        } else if path.exists() && !self.save_image_overwrite {
                            self.save_image_overwrite = true;
                            self.save_image_error =
                                Some("File already exists. Press Save again to replace it.".into());
                        } else {
                            self.save_image_pending = true;
                            self.save_image_dialog = false;
                            self.save_image_error = None;
                            self.save_image_capture_requested = true;
                            context.request_repaint();
                        }
                    }
                    if ui
                        .add_enabled(!self.save_image_pending, egui::Button::new("Cancel"))
                        .clicked()
                    {
                        self.save_image_dialog = false;
                    }
                });
            });
        self.save_image_dialog = open && self.save_image_dialog;
    }
}

fn write_screenshot(path: &std::path::Path, image: &egui::ColorImage) -> Result<(), String> {
    let [width, height] = image.size;
    if width == 0 || height == 0 || image.pixels.len() != width.saturating_mul(height) {
        return Err("Screenshot image has invalid dimensions.".into());
    }
    match path
        .extension()
        .and_then(|ext| ext.to_str())
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("png") => {
            let file = std::fs::File::create(path).map_err(|error| error.to_string())?;
            let mut encoder = png::Encoder::new(file, width as u32, height as u32);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            let mut writer = encoder.write_header().map_err(|error| error.to_string())?;
            let mut rgba = Vec::with_capacity(image.pixels.len() * 4);
            for pixel in &image.pixels {
                rgba.extend_from_slice(&pixel.to_array());
            }
            writer
                .write_image_data(&rgba)
                .map_err(|error| error.to_string())
        }
        Some("bmp") => write_bmp(path, width, height, image),
        _ => Err("Choose a .png or .bmp file path.".into()),
    }
}

fn write_bmp(
    path: &std::path::Path,
    width: usize,
    height: usize,
    image: &egui::ColorImage,
) -> Result<(), String> {
    use std::io::Write;
    let byte_count = width
        .checked_mul(height)
        .and_then(|pixels| pixels.checked_mul(4))
        .and_then(|bytes| u32::try_from(bytes).ok())
        .ok_or_else(|| "Screenshot is too large for BMP format.".to_string())?;
    let width_i32 = i32::try_from(width).map_err(|_| "Screenshot is too wide for BMP format.")?;
    let height_i32 = i32::try_from(height).map_err(|_| "Screenshot is too tall for BMP format.")?;
    let mut file = std::fs::File::create(path).map_err(|error| error.to_string())?;
    file.write_all(b"BM").map_err(|error| error.to_string())?;
    file.write_all(&(54u32 + byte_count).to_le_bytes())
        .map_err(|error| error.to_string())?;
    file.write_all(&[0; 4]).map_err(|error| error.to_string())?;
    file.write_all(&54u32.to_le_bytes())
        .map_err(|error| error.to_string())?;
    file.write_all(&40u32.to_le_bytes())
        .map_err(|error| error.to_string())?;
    file.write_all(&width_i32.to_le_bytes())
        .map_err(|error| error.to_string())?;
    file.write_all(&height_i32.to_le_bytes())
        .map_err(|error| error.to_string())?;
    file.write_all(&1u16.to_le_bytes())
        .map_err(|error| error.to_string())?;
    file.write_all(&32u16.to_le_bytes())
        .map_err(|error| error.to_string())?;
    file.write_all(&0u32.to_le_bytes())
        .map_err(|error| error.to_string())?;
    file.write_all(&byte_count.to_le_bytes())
        .map_err(|error| error.to_string())?;
    file.write_all(&2835i32.to_le_bytes())
        .map_err(|error| error.to_string())?;
    file.write_all(&2835i32.to_le_bytes())
        .map_err(|error| error.to_string())?;
    file.write_all(&[0; 8]).map_err(|error| error.to_string())?;
    for row in (0..height).rev() {
        let start = row * width;
        for pixel in &image.pixels[start..start + width] {
            let [red, green, blue, _] = pixel.to_array();
            file.write_all(&[blue, green, red, 255])
                .map_err(|error| error.to_string())?;
        }
    }
    Ok(())
}

fn smartctl_output(device: &str, args: &[&str], ascii_view: bool) -> String {
    let path = format!("/dev/{device}");
    match std::process::Command::new("smartctl")
        .args(args)
        .arg(path)
        .output()
    {
        Ok(output) => {
            let text = if output.stdout.is_empty() {
                String::from_utf8_lossy(&output.stderr).into_owned()
            } else {
                String::from_utf8_lossy(&output.stdout).into_owned()
            };
            if text.is_empty() {
                format!(
                    "smartctl returned no output (exit status {}).\n",
                    output.status
                )
            } else if ascii_view {
                append_ascii_columns(&text)
            } else {
                text
            }
        }
        Err(error) => format!("Could not run smartctl: {error}\n"),
    }
}

fn append_ascii_columns(text: &str) -> String {
    let mut output = String::with_capacity(text.len());
    for line in text.lines() {
        output.push_str(line);
        if let Some((offset, bytes)) = line.split_once(':')
            && offset
                .trim()
                .trim_start_matches("0x")
                .chars()
                .all(|ch| ch.is_ascii_hexdigit())
        {
            let decoded = bytes
                .split_whitespace()
                .filter_map(|part| {
                    let byte = part.strip_prefix("0x").unwrap_or(part);
                    (byte.len() == 2)
                        .then(|| u8::from_str_radix(byte, 16).ok())
                        .flatten()
                })
                .collect::<Vec<_>>();
            if decoded.len() >= 8 {
                output.push_str("  ");
                for byte in decoded {
                    output.push(if byte.is_ascii_graphic() {
                        byte as char
                    } else {
                        '.'
                    });
                }
            }
        }
        output.push('\n');
    }
    output
}

fn drive_view(
    ui: &mut egui::Ui,
    drive: &Drive,
    settings: &DriveSettings,
    fahrenheit: bool,
    hide_serial: bool,
    green_mode: bool,
    raw_format: RawFormat,
    hide_smart_info: bool,
) -> Option<DriveViewAction> {
    let mut open_setting = None;
    ui.vertical(|ui| {
        let (status, color) = status_label_with_settings(drive, settings, green_mode);
        let status_text = drive
            .health_percent
            .map(|life| format!("{status}\n{life} %"))
            .unwrap_or_else(|| status.to_string());
        ui.horizontal(|ui| {
            if ui
                .add_sized(Vec2::new(28.0, 24.0), egui::Button::new("‹"))
                .clicked()
            {
                open_setting = Some(DriveViewAction::PreviousDrive);
            }
            let model_width = (ui.available_width() - 68.0).max(0.0);
            ui.add_sized(
                Vec2::new(model_width, 32.0),
                egui::Label::new(RichText::new(&drive.model).strong().size(16.0))
                    .truncate()
                    .selectable(false),
            );
            if ui
                .add_sized(Vec2::new(28.0, 24.0), egui::Button::new("›"))
                .clicked()
            {
                open_setting = Some(DriveViewAction::NextDrive);
            }
        });
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
                        ui.add_space(4.0);
                        if ui
                            .add_sized(
                                Vec2::new(100.0, 60.0),
                                egui::Button::new(RichText::new(status_text).strong().color(color)),
                            )
                            .clicked()
                        {
                            open_setting = Some(DriveViewAction::HealthSetting);
                        }
                        ui.add_space(12.0);
                        ui.label(RichText::new("Temperature").weak().size(11.0));
                        ui.add_space(4.0);
                        let temperature = drive
                            .temperature_c
                            .map(|value| show_temperature(value, fahrenheit))
                            .unwrap_or_else(|| if fahrenheit { "-- °F" } else { "-- °C" }.into());
                        let temperature_color = match drive.temperature_c {
                            Some(temperature)
                                if settings.temperature_alarm > 0
                                    && temperature >= i64::from(settings.temperature_alarm) =>
                            {
                                Color32::from_rgb(234, 77, 77)
                            }
                            Some(_) if green_mode => Color32::from_rgb(87, 205, 95),
                            Some(_) => Color32::from_rgb(55, 181, 118),
                            None => Color32::from_rgb(145, 150, 160),
                        };
                        let temperature_button = ui.add_sized(
                            Vec2::new(100.0, 40.0),
                            egui::Button::new(
                                RichText::new(temperature)
                                    .size(18.0)
                                    .strong()
                                    .color(temperature_color),
                            ),
                        );
                        let temperature_button =
                            if let Some(tooltip) = nvme_temperature_tooltip(drive, fahrenheit) {
                                temperature_button.on_hover_text(tooltip)
                            } else {
                                temperature_button
                            };
                        if temperature_button.clicked() {
                            open_setting = Some(DriveViewAction::TemperatureSetting);
                        }
                    },
                );

                ui.separator();
                ui.columns(2, |columns| {
                    metadata_column(&mut columns[0], drive, hide_serial);
                    metadata_column_secondary(&mut columns[1], drive);
                });
            },
        );

        if !hide_smart_info {
            ui.separator();
            if !drive.ata_attributes.is_empty() {
                ui.label(RichText::new("S.M.A.R.T.").strong().size(12.0));
                ata_attribute_table(ui, drive, settings, raw_format);
            } else if !drive.nvme_metrics.is_empty() {
                ui.label(RichText::new("NVMe S.M.A.R.T.").strong().size(12.0));
                nvme_table(ui, drive, settings, green_mode, raw_format);
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
        }
    });
    open_setting
}

fn metadata_column(ui: &mut egui::Ui, drive: &Drive, hide_serial: bool) {
    Grid::new("metadata_left")
        .num_columns(2)
        .spacing(Vec2::new(12.0, 4.0))
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
            metadata(ui, "Feature", "Unavailable");
        });
}

fn metadata_column_secondary(ui: &mut egui::Ui, drive: &Drive) {
    Grid::new("metadata_right")
        .num_columns(2)
        .spacing(Vec2::new(12.0, 4.0))
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
                    let ssd = drive
                        .rotation_rate
                        .as_deref()
                        .is_some_and(|rate| rate == "SSD" || rate == "0");
                    for attribute in &drive.ata_attributes {
                        let caution = ata_raw_caution(attribute, settings, ssd);
                        let failed = ata_attribute_failed(attribute, ssd);
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

fn nvme_table(
    ui: &mut egui::Ui,
    drive: &Drive,
    settings: &DriveSettings,
    green_mode: bool,
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
            Grid::new("nvme_smart_table")
                .num_columns(headers.len())
                .spacing(Vec2::new(10.0, 3.0))
                .striped(true)
                .show(ui, |ui| {
                    for heading in headers {
                        ui.label(RichText::new(heading).weak().size(11.0));
                    }
                    ui.end_row();
                    for metric in &drive.nvme_metrics {
                        let (label, color) =
                            nvme_metric_status(drive, metric, settings, green_mode);
                        ui.colored_label(color, label);
                        ui.label(format!("{:02X}", metric.id));
                        ui.label(&metric.name);
                        ui.label("0");
                        ui.label("0");
                        ui.label("0");
                        ui.label(
                            format_nvme_raw(metric, raw_format)
                                .unwrap_or_else(|| metric.value.clone()),
                        );
                        ui.end_row();
                    }
                });
        });
}

fn format_nvme_raw(metric: &storage::Metric, format: RawFormat) -> Option<String> {
    let value = if metric.id == 0x01 {
        u64::from_str_radix(metric.value.strip_prefix("0x")?, 16).ok()?
    } else {
        let numeric = metric
            .value
            .split_whitespace()
            .next()?
            .trim_end_matches('%');
        if metric.id == 0x02 || (0x12..=0x19).contains(&metric.id) {
            u64::try_from(numeric.parse::<i64>().ok()? + 273).ok()?
        } else {
            numeric.parse::<u64>().ok()?
        }
    } & 0x0000_FFFF_FFFF_FFFF;

    let bytes = value.to_le_bytes();
    Some(match format {
        RawFormat::Hex => format!(
            "{:02X}{:02X}{:02X}{:02X}{:02X}{:02X}",
            bytes[5], bytes[4], bytes[3], bytes[2], bytes[1], bytes[0]
        ),
        RawFormat::Decimal => value.to_string(),
        RawFormat::TwoByte => format!(
            "{} {} {}",
            u16::from_le_bytes([bytes[4], bytes[5]]),
            u16::from_le_bytes([bytes[2], bytes[3]]),
            u16::from_le_bytes([bytes[0], bytes[1]])
        ),
        RawFormat::OneByte => format!(
            "{} {} {} {} {} {}",
            bytes[5], bytes[4], bytes[3], bytes[2], bytes[1], bytes[0]
        ),
    })
}

fn nvme_temperature_tooltip(drive: &Drive, fahrenheit: bool) -> Option<String> {
    let mut lines = Vec::new();
    for metric in &drive.nvme_metrics {
        if !(0x12..=0x19).contains(&metric.id) {
            continue;
        }
        let celsius = metric
            .value
            .split_whitespace()
            .next()?
            .parse::<i64>()
            .ok()?;
        let temperature = if fahrenheit {
            format!("{} °F", celsius * 9 / 5 + 32)
        } else {
            format!("{celsius} °C")
        };
        lines.push(format!(
            "Temperature sensor {}: {temperature}",
            metric.id - 0x11
        ));
    }
    (!lines.is_empty()).then(|| lines.join("\n"))
}

fn nvme_metric_status(
    drive: &Drive,
    metric: &storage::Metric,
    settings: &DriveSettings,
    green_mode: bool,
) -> (&'static str, Color32) {
    let unknown = ("UNKNOWN", Color32::GRAY);
    let good = (
        "GOOD",
        if green_mode {
            Color32::from_rgb(87, 205, 95)
        } else {
            Color32::from_rgb(63, 184, 107)
        },
    );
    let caution = ("CAUTION", Color32::from_rgb(224, 170, 55));
    let bad = ("BAD", Color32::from_rgb(234, 77, 77));

    let model = drive.model.to_ascii_lowercase();
    if ["parallels", "vmware", "qemu"]
        .iter()
        .any(|virtualizer| model.starts_with(virtualizer))
    {
        return unknown;
    }

    match metric.id {
        0x01 => match drive.nvme_critical_warning {
            Some(0) => good,
            Some(_) => bad,
            None => unknown,
        },
        0x02 => match drive.temperature_c {
            Some(temperature) if settings.temperature_alarm > 0 => {
                if temperature >= i64::from(settings.temperature_alarm) {
                    bad
                } else {
                    good
                }
            }
            Some(_) => good,
            None => unknown,
        },
        0x03 => match (
            drive.nvme_available_spare,
            drive.nvme_available_spare_threshold,
        ) {
            (Some(spare), Some(threshold)) if threshold > 100 || (spare == 0 && threshold == 0) => {
                good
            }
            (Some(spare), Some(threshold)) if spare < threshold => bad,
            (Some(spare), Some(threshold)) if spare == threshold && threshold != 100 => caution,
            (Some(_), Some(_)) => good,
            _ => unknown,
        },
        0x05 => match drive.health_percent {
            Some(life) if i32::from(life) <= settings.threshold_ff => caution,
            Some(_) => good,
            None => unknown,
        },
        _ if drive.smart_available => good,
        _ => unknown,
    }
}

fn is_normalized_health_attribute(id: u8) -> bool {
    matches!(
        id,
        0x01..=0x0D
            | 0x16
            | 0xBB..=0xBD
            | 0xBF..=0xC1
            | 0xC3..=0xD1
            | 0xD3..=0xD4
            | 0xDC..=0xE4
            | 0xE6..=0xE7
            | 0xF0
            | 0xFA
            | 0xFE
    )
}

fn ata_attribute_failed(attribute: &storage::AtaSmartAttribute, ssd: bool) -> bool {
    if attribute.id == 0xC2 {
        return false;
    }
    attribute.failed
        || (attribute.threshold > 0
            && (ssd || is_normalized_health_attribute(attribute.id))
            && attribute.current < attribute.threshold)
}

fn ata_raw_caution(
    attribute: &storage::AtaSmartAttribute,
    settings: &DriveSettings,
    ssd: bool,
) -> bool {
    if ssd {
        return false;
    }
    let threshold = match attribute.id {
        0x05 => settings.threshold_05,
        0xC5 => settings.threshold_c5,
        0xC6 => settings.threshold_c6,
        _ => return false,
    };
    threshold > 0
        && attribute
            .raw_value
            .is_some_and(|raw| raw & 0xFFFF_FFFF != 0xFFFF_FFFF && (raw as u16) >= threshold as u16)
}

fn status_label_with_settings(
    drive: &Drive,
    settings: &DriveSettings,
    green_mode: bool,
) -> (&'static str, Color32) {
    let bad = ("Bad", Color32::from_rgb(234, 77, 77));
    let caution_status = ("Caution", Color32::from_rgb(224, 170, 55));
    let unknown = ("Unknown", Color32::from_rgb(145, 150, 160));
    let good = if green_mode {
        ("Good", Color32::from_rgb(87, 205, 95))
    } else {
        ("Good", Color32::from_rgb(55, 181, 118))
    };

    if drive.kind.eq_ignore_ascii_case("NVMe") || drive.nvme_critical_warning.is_some() {
        let model = drive.model.to_ascii_lowercase();
        if ["parallels", "vmware", "qemu"]
            .iter()
            .any(|virtualizer| model.starts_with(virtualizer))
        {
            return unknown;
        }
        if drive
            .nvme_critical_warning
            .is_some_and(|warning| warning != 0)
            || drive.smart_status == Some(false)
        {
            return bad;
        }
        let spare = drive.nvme_available_spare;
        let spare_threshold = drive.nvme_available_spare_threshold;
        if let (Some(spare), Some(threshold)) = (spare, spare_threshold)
            && threshold > 0
            && threshold <= 100
        {
            if spare < threshold {
                return bad;
            }
            if spare == threshold && threshold != 100 {
                return caution_status;
            }
        }
        if drive
            .health_percent
            .is_some_and(|life| i32::from(life) <= settings.threshold_ff)
        {
            return caution_status;
        }
        if drive.smart_status == Some(true) || drive.nvme_critical_warning.is_some() {
            return good;
        }
        return unknown;
    }

    if drive.smart_status == Some(false) {
        return bad;
    }

    let mut caution = false;
    let ssd = drive
        .rotation_rate
        .as_deref()
        .is_some_and(|rate| rate == "SSD" || rate == "0");
    let mut has_health_indicator = false;
    for attribute in &drive.ata_attributes {
        let raw_health_id = matches!(attribute.id, 0x05 | 0xC5 | 0xC6);

        if raw_health_id && !ssd {
            has_health_indicator = true;
            caution |= ata_raw_caution(attribute, settings, ssd);
        }

        if ssd && attribute.threshold > 0 {
            has_health_indicator = true;
        }

        if ata_attribute_failed(attribute, ssd) {
            return bad;
        }
    }

    if !has_health_indicator || !drive.smart_available {
        return unknown;
    }
    if caution { caution_status } else { good }
}

fn status_severity(status: &str) -> u8 {
    match status {
        "Good" => 1,
        "Caution" => 2,
        "Bad" => 3,
        _ => 0,
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
