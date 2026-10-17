use serde::Deserialize;
use std::{fs, path::Path, process::Command};

#[derive(Clone, Debug, Default)]
pub struct Drive {
    pub name: String,
    pub model: String,
    pub firmware: Option<String>,
    pub serial: String,
    pub kind: String,
    pub capacity: u64,
    pub interface: Option<String>,
    pub transfer_mode: Option<String>,
    pub drive_map: Vec<String>,
    pub rotation_rate: Option<String>,
    pub power_cycles: Option<u64>,
    pub host_reads_gb: Option<u64>,
    pub host_writes_gb: Option<u64>,
    pub features: Vec<String>,
    pub ncq_supported: bool,
    pub trim_supported: bool,
    pub volatile_write_cache: bool,
    pub temperature_c: Option<i64>,
    pub power_on_hours: Option<u64>,
    pub health_percent: Option<u8>,
    pub smart_status: Option<bool>,
    pub nvme_critical_warning: Option<u8>,
    pub nvme_available_spare: Option<u8>,
    pub nvme_available_spare_threshold: Option<u8>,
    pub assessment: String,
    pub smart_available: bool,
    pub note: Option<String>,
    pub nvme_metrics: Vec<Metric>,
    pub ata_attributes: Vec<AtaSmartAttribute>,
}

pub type DeviceSignature = Vec<(String, String, u64)>;

#[derive(Clone, Debug)]
pub struct Metric {
    pub id: u8,
    pub name: String,
    pub value: String,
}

#[derive(Clone, Debug)]
pub struct AtaSmartAttribute {
    pub id: u8,
    pub name: String,
    pub current: u16,
    pub worst: u16,
    pub threshold: u16,
    pub raw: String,
    pub raw_value: Option<u64>,
    pub failed: bool,
}

#[derive(Deserialize)]
struct SmartctlOutput {
    model_name: Option<String>,
    model_family: Option<String>,
    firmware_version: Option<String>,
    serial_number: Option<String>,
    user_capacity: Option<Capacity>,
    rotation_rate: Option<serde_json::Value>,
    ata_version: Option<VersionInfo>,
    sata_interface_speed: Option<SataInterfaceSpeed>,
    temperature: Option<Temperature>,
    power_on_time: Option<PowerOnTime>,
    smart_status: Option<SmartStatus>,
    nvme_smart_health_information_log: Option<NvmeHealth>,
    ata_smart_attributes: Option<AtaAttributes>,
    trim: Option<TrimSupport>,
    ata_apm: Option<FeatureSetting>,
    ata_aam: Option<FeatureSetting>,
    ata_smart_data: Option<AtaSmartData>,
}

#[derive(Deserialize)]
struct Capacity {
    bytes: Option<u64>,
}
#[derive(Deserialize)]
struct VersionInfo {
    string: Option<String>,
}
#[derive(Deserialize)]
struct SataInterfaceSpeed {
    current: Option<VersionInfo>,
}
#[derive(Deserialize)]
struct Temperature {
    current: Option<i64>,
}
#[derive(Deserialize)]
struct PowerOnTime {
    hours: Option<u64>,
}
#[derive(Deserialize)]
struct SmartStatus {
    passed: Option<bool>,
}
#[derive(Deserialize)]
struct TrimSupport {
    supported: Option<bool>,
}
#[derive(Deserialize)]
struct FeatureSetting {}
#[derive(Deserialize)]
struct AtaSmartData {
    capabilities: Option<AtaSmartCapabilities>,
}
#[derive(Deserialize)]
struct AtaSmartCapabilities {
    gp_logging_supported: Option<bool>,
}
#[derive(Deserialize)]
struct NvmeHealth {
    critical_warning: Option<u8>,
    temperature: Option<i64>,
    available_spare: Option<u8>,
    available_spare_threshold: Option<u8>,
    power_on_hours: Option<u64>,
    percentage_used: Option<u8>,
    data_units_read: Option<u64>,
    data_units_written: Option<u64>,
    host_reads: Option<u64>,
    host_writes: Option<u64>,
    controller_busy_time: Option<u64>,
    power_cycles: Option<u64>,
    unsafe_shutdowns: Option<u64>,
    media_errors: Option<u64>,
    num_err_log_entries: Option<u64>,
    warning_temp_time: Option<u64>,
    critical_comp_time: Option<u64>,
    temperature_sensors: Option<Vec<Option<i64>>>,
    thermal_temp1_transition_count: Option<u64>,
    thermal_temp2_transition_count: Option<u64>,
    thermal_temp1_total_time: Option<u64>,
    thermal_temp2_total_time: Option<u64>,
}
#[derive(Deserialize)]
struct AtaAttributes {
    table: Option<Vec<AtaAttribute>>,
}
#[derive(Deserialize)]
struct AtaAttribute {
    id: Option<u8>,
    name: Option<String>,
    value: Option<u16>,
    worst: Option<u16>,
    thresh: Option<u16>,
    when_failed: Option<String>,
    raw: Option<RawValue>,
}
#[derive(Deserialize)]
struct RawValue {
    value: Option<u64>,
    string: Option<String>,
}

pub fn discover() -> Vec<Drive> {
    let Ok(entries) = fs::read_dir("/sys/block") else {
        return Vec::new();
    };
    let mut drives = entries
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with("loop") || name.starts_with("ram") || name.starts_with("zram") {
                return None;
            }
            let root = entry.path();
            let bytes = read_u64(root.join("size")).unwrap_or(0).saturating_mul(512);
            let model = read_trimmed(root.join("device/model"))
                .or_else(|| read_trimmed(root.join("device/name")))
                .unwrap_or_else(|| "Unknown model".into());
            let kind = if name.starts_with("nvme") {
                "NVMe"
            } else if name.starts_with("sd") {
                "ATA / SCSI"
            } else if name.starts_with("mmc") {
                "MMC"
            } else {
                "Block device"
            };
            let mut drive = Drive {
                name: name.clone(),
                model,
                kind: kind.into(),
                capacity: bytes,
                ncq_supported: read_u64(root.join("device/queue_depth"))
                    .is_some_and(|depth| depth > 1),
                trim_supported: read_u64(root.join("queue/discard_max_bytes"))
                    .is_some_and(|bytes| bytes > 0),
                volatile_write_cache: read_trimmed(root.join("queue/write_cache"))
                    .is_some_and(|mode| mode.eq_ignore_ascii_case("write back")),
                ..Drive::default()
            };
            drive.drive_map = mounted_paths(&name);
            drive.rotation_rate = read_trimmed(root.join("queue/rotational")).map(|value| {
                if value == "1" {
                    "Rotational".into()
                } else {
                    "SSD".into()
                }
            });
            if let Some(smart) = read_smartctl(&name) {
                merge_smart(&mut drive, smart);
            }
            Some(drive)
        })
        .collect::<Vec<_>>();
    drives.sort_by(|a, b| a.name.cmp(&b.name));
    drives
}

pub fn device_signature() -> DeviceSignature {
    let Ok(entries) = fs::read_dir("/sys/block") else {
        return Vec::new();
    };
    let mut signature = entries
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with("loop") || name.starts_with("ram") || name.starts_with("zram") {
                return None;
            }
            let root = entry.path();
            let capacity = read_u64(root.join("size")).unwrap_or(0).saturating_mul(512);
            let model = read_trimmed(root.join("device/model"))
                .or_else(|| read_trimmed(root.join("device/name")))
                .unwrap_or_default();
            Some((name, model, capacity))
        })
        .collect::<DeviceSignature>();
    signature.sort_by(|left, right| left.0.cmp(&right.0));
    signature
}

#[derive(Deserialize)]
struct SmartctlEnvelope {
    device: Option<DeviceInfo>,
    smartctl: Option<SmartctlInfo>,
    #[serde(flatten)]
    data: SmartctlOutput,
}
#[derive(Deserialize)]
struct DeviceInfo {
    name: Option<String>,
}
#[derive(Deserialize)]
struct SmartctlInfo {
    exit_status: Option<i32>,
    messages: Option<Vec<SmartctlMessage>>,
    output: Option<Vec<String>>,
}
#[derive(Deserialize)]
struct SmartctlMessage {
    string: Option<String>,
    severity: Option<String>,
}

fn read_smartctl(device: &str) -> Option<SmartctlEnvelope> {
    let path = format!("/dev/{device}");
    let output = Command::new("smartctl")
        .args(["-x", "--json=o", &path])
        .output()
        .ok()?;
    // smartctl uses a nonzero status to report SMART warnings and health failures;
    // JSON remains useful, so decode stdout regardless of the exit code.
    let mut envelope =
        if let Ok(envelope) = serde_json::from_slice::<SmartctlEnvelope>(&output.stdout) {
            envelope
        } else {
            let fallback = Command::new("smartctl")
                .args(["-a", "-j", &path])
                .output()
                .ok()?;
            serde_json::from_slice(&fallback.stdout).ok()?
        };
    let text = envelope
        .smartctl
        .as_ref()
        .and_then(|smartctl| smartctl.output.as_ref())
        .map(|lines| lines.join("\n"))
        .unwrap_or_default();
    if envelope.data.nvme_smart_health_information_log.is_some() && !text.is_empty() {
        if let Some(nvme) = envelope.data.nvme_smart_health_information_log.as_mut() {
            nvme.thermal_temp1_transition_count =
                smartctl_nvme_value(&text, "Thermal Temp. 1 Transition Count");
            nvme.thermal_temp2_transition_count =
                smartctl_nvme_value(&text, "Thermal Temp. 2 Transition Count");
            nvme.thermal_temp1_total_time =
                smartctl_nvme_value(&text, "Thermal Temp. 1 Total Time");
            nvme.thermal_temp2_total_time =
                smartctl_nvme_value(&text, "Thermal Temp. 2 Total Time");
        }
    }
    Some(envelope)
}

fn smartctl_nvme_value(output: &str, label: &str) -> Option<u64> {
    output.lines().find_map(|line| {
        let (name, value) = line.split_once(':')?;
        name.trim()
            .eq_ignore_ascii_case(label)
            .then(|| value.trim().replace(',', "").parse().ok())
            .flatten()
    })
}

fn merge_smart(drive: &mut Drive, result: SmartctlEnvelope) {
    let data = result.data;
    let is_nvme = data.nvme_smart_health_information_log.is_some();
    let is_ata = data.ata_version.is_some();
    let supports_apm = data.ata_apm.is_some();
    let supports_aam = data.ata_aam.is_some();
    let supports_trim = data
        .trim
        .as_ref()
        .and_then(|trim| trim.supported)
        .unwrap_or(false);
    let supports_gpl = data
        .ata_smart_data
        .as_ref()
        .and_then(|smart| smart.capabilities.as_ref())
        .and_then(|capabilities| capabilities.gp_logging_supported)
        .unwrap_or(false);
    drive.trim_supported |= supports_trim;
    drive.model = data
        .model_name
        .or(data.model_family)
        .unwrap_or_else(|| drive.model.clone());
    drive.serial = data.serial_number.unwrap_or_default();
    drive.firmware = data.firmware_version;
    drive.capacity = data
        .user_capacity
        .and_then(|v| v.bytes)
        .filter(|v| *v > 0)
        .unwrap_or(drive.capacity);
    drive.temperature_c = data.temperature.and_then(|v| v.current);
    drive.power_on_hours = data.power_on_time.and_then(|v| v.hours);
    drive.smart_status = data.smart_status.and_then(|v| v.passed);
    drive.interface = data
        .ata_version
        .and_then(|version| version.string)
        .or_else(|| Some(drive.kind.clone()));
    drive.transfer_mode = data
        .sata_interface_speed
        .and_then(|speed| speed.current)
        .and_then(|speed| speed.string);
    if let Some(rotation_rate) = data.rotation_rate {
        drive.rotation_rate = Some(if let Some(rate) = rotation_rate.as_u64() {
            if rate == 0 {
                "SSD".into()
            } else {
                format!("{rate} RPM")
            }
        } else {
            rotation_rate.as_str().unwrap_or_default().to_string()
        });
    }
    let mut has_nvme_smart_data = false;
    if let Some(nvme) = data.nvme_smart_health_information_log {
        has_nvme_smart_data = true;
        drive.nvme_critical_warning = nvme.critical_warning;
        drive.nvme_available_spare = nvme.available_spare;
        drive.nvme_available_spare_threshold = nvme.available_spare_threshold;
        drive.power_cycles = nvme.power_cycles.or(drive.power_cycles);
        drive.host_reads_gb = nvme.data_units_read.map(nvme_data_units_to_gb);
        drive.host_writes_gb = nvme.data_units_written.map(nvme_data_units_to_gb);
        if nvme.critical_warning.is_some_and(|warning| warning != 0) {
            drive.smart_status = Some(false);
        } else if drive.smart_status.is_none() {
            drive.smart_status = Some(true);
        }
        drive.temperature_c = nvme.temperature.or(drive.temperature_c);
        drive.power_on_hours = nvme.power_on_hours.or(drive.power_on_hours);
        drive.health_percent = nvme
            .percentage_used
            .map(|used| 100u8.saturating_sub(used.min(100)));
        let metrics = [
            (
                0x01,
                "Critical warning",
                nvme.critical_warning.map(|value| format!("0x{value:02X}")),
            ),
            (
                0x02,
                "Temperature",
                nvme.temperature.map(|value| format!("{value} °C")),
            ),
            (
                0x03,
                "Available spare",
                nvme.available_spare.map(|v| format!("{v}%")),
            ),
            (
                0x04,
                "Available spare threshold",
                nvme.available_spare_threshold.map(|v| format!("{v}%")),
            ),
            (
                0x05,
                "Percentage used",
                nvme.percentage_used.map(|v| format!("{v}%")),
            ),
            (
                0x06,
                "Data units read",
                nvme.data_units_read.map(|v| v.to_string()),
            ),
            (
                0x07,
                "Data units written",
                nvme.data_units_written.map(|v| v.to_string()),
            ),
            (
                0x08,
                "Host read commands",
                nvme.host_reads.map(|v| v.to_string()),
            ),
            (
                0x09,
                "Host write commands",
                nvme.host_writes.map(|v| v.to_string()),
            ),
            (
                0x0A,
                "Controller busy time",
                nvme.controller_busy_time.map(|v| format!("{v} minutes")),
            ),
            (
                0x0B,
                "Power cycles",
                nvme.power_cycles.map(|v| v.to_string()),
            ),
            (
                0x0C,
                "Power on hours",
                nvme.power_on_hours.map(|v| format!("{v} hours")),
            ),
            (
                0x0D,
                "Unsafe shutdowns",
                nvme.unsafe_shutdowns.map(|v| v.to_string()),
            ),
            (
                0x0E,
                "Media errors",
                nvme.media_errors.map(|v| v.to_string()),
            ),
            (
                0x0F,
                "Error log entries",
                nvme.num_err_log_entries.map(|v| v.to_string()),
            ),
        ];
        drive
            .nvme_metrics
            .extend(metrics.into_iter().filter_map(|(id, name, value)| {
                value.map(|value| Metric {
                    id,
                    name: name.into(),
                    value,
                })
            }));
        for (id, name, value) in [
            (
                0x10,
                "Warning composite temperature time",
                nvme.warning_temp_time,
            ),
            (
                0x11,
                "Critical composite temperature time",
                nvme.critical_comp_time,
            ),
        ] {
            if let Some(value) = value {
                drive.nvme_metrics.push(Metric {
                    id,
                    name: name.into(),
                    value: format!("{value} minutes"),
                });
            }
        }
        if let Some(sensors) = nvme.temperature_sensors {
            drive
                .nvme_metrics
                .extend(sensors.into_iter().take(8).enumerate().filter_map(
                    |(index, temperature)| {
                        temperature
                            .filter(|temperature| *temperature != 0)
                            .map(|temperature| Metric {
                                id: 0x12 + index as u8,
                                name: format!("Temperature sensor {}", index + 1),
                                value: format!("{temperature} °C"),
                            })
                    },
                ));
        }
        for (id, name, value) in [
            (
                0x1A,
                "Thermal temperature 1 transition count",
                nvme.thermal_temp1_transition_count,
            ),
            (
                0x1B,
                "Thermal temperature 2 transition count",
                nvme.thermal_temp2_transition_count,
            ),
            (
                0x1C,
                "Thermal temperature 1 total time",
                nvme.thermal_temp1_total_time,
            ),
            (
                0x1D,
                "Thermal temperature 2 total time",
                nvme.thermal_temp2_total_time,
            ),
        ] {
            if let Some(value) = value {
                drive.nvme_metrics.push(Metric {
                    id,
                    name: name.into(),
                    value: value.to_string(),
                });
            }
        }
    }
    if let Some(attributes) = data.ata_smart_attributes.and_then(|v| v.table) {
        for attribute in attributes {
            if let (Some(id), Some(name)) = (attribute.id, attribute.name.as_ref()) {
                let raw = attribute.raw.as_ref();
                drive.ata_attributes.push(AtaSmartAttribute {
                    id,
                    name: name.clone(),
                    current: attribute.value.unwrap_or_default(),
                    worst: attribute.worst.unwrap_or_default(),
                    threshold: attribute.thresh.unwrap_or_default(),
                    raw: raw
                        .and_then(|raw| raw.string.clone())
                        .or_else(|| raw.and_then(|raw| raw.value).map(|v| v.to_string()))
                        .unwrap_or_else(|| "-".into()),
                    raw_value: raw.and_then(|raw| raw.value),
                    failed: attribute
                        .when_failed
                        .as_deref()
                        .is_some_and(|status| status.eq_ignore_ascii_case("FAILING_NOW")),
                });
            }
            let raw = attribute.raw;
            let value = raw.as_ref().and_then(|raw| raw.value);
            match attribute.id {
                Some(9) => drive.power_on_hours = value.or(drive.power_on_hours),
                Some(12) => drive.power_cycles = value.or(drive.power_cycles),
                Some(194 | 190) => {
                    let parsed = raw
                        .as_ref()
                        .and_then(|raw| raw.string.as_deref())
                        .and_then(|text| text.split_whitespace().next())
                        .and_then(|text| text.parse::<i64>().ok());
                    drive.temperature_c = parsed
                        .or_else(|| value.map(|v| (v & 0xff) as i64))
                        .or(drive.temperature_c);
                }
                _ => {}
            }
        }
    }
    if drive.smart_status.is_none() && !drive.ata_attributes.is_empty() {
        drive.smart_status = Some(!drive.ata_attributes.iter().any(|attribute| {
            attribute.failed || (attribute.threshold > 0 && attribute.current < attribute.threshold)
        }));
    }
    drive.smart_available = !drive.ata_attributes.is_empty() || has_nvme_smart_data;
    drive.features.clear();
    if drive.smart_available {
        drive.features.push("S.M.A.R.T.".into());
    }
    if is_nvme {
        if drive.trim_supported {
            drive.features.push("TRIM".into());
        }
        if drive.volatile_write_cache {
            drive.features.push("VolatileWriteCache".into());
        }
    } else if is_ata {
        if supports_apm {
            drive.features.push("APM".into());
        }
        if supports_aam {
            drive.features.push("AAM".into());
        }
        if drive.ncq_supported {
            drive.features.push("NCQ".into());
        }
        if drive.trim_supported {
            drive.features.push("TRIM".into());
        }
        if supports_gpl {
            drive.features.push("GPL".into());
        }
    }
    drive.assessment = match drive.smart_status {
        Some(true) => "Good".into(),
        Some(false) => "Warning".into(),
        None if drive.smart_available => "SMART data available".into(),
        None => "SMART unavailable".into(),
    };
    let smartctl = result.smartctl;
    let status = smartctl
        .as_ref()
        .and_then(|v| v.exit_status)
        .unwrap_or_default();
    drive.note = smartctl
        .and_then(|v| v.messages)
        .and_then(|messages| {
            messages
                .into_iter()
                .find(|message| message.severity.as_deref() == Some("error"))
        })
        .and_then(|message| message.string);
    if status & 2 != 0 && drive.note.is_none() {
        drive.note = Some("The device could not be opened for SMART queries.".into());
    }
    let _ = result.device.and_then(|v| v.name);
}

fn nvme_data_units_to_gb(data_units: u64) -> u64 {
    data_units.wrapping_mul(1000) >> 21
}

fn mounted_paths(device: &str) -> Vec<String> {
    let Ok(device_path) = fs::canonicalize(format!("/sys/block/{device}")) else {
        return Vec::new();
    };
    let Ok(mounts) = fs::read_to_string("/proc/self/mounts") else {
        return Vec::new();
    };
    let mut paths = mounts
        .lines()
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            let source = fields.next()?;
            let mount_point = fields.next()?;
            if !source.starts_with("/dev/") {
                return None;
            }
            let source_name = source.rsplit('/').next()?;
            let source_sysfs = fs::canonicalize(format!("/sys/class/block/{source_name}")).ok()?;
            if source_sysfs == device_path || source_sysfs.starts_with(&device_path) {
                Some(mount_point.replace("\\040", " "))
            } else {
                None
            }
        })
        .collect::<Vec<_>>();
    paths.sort();
    paths.dedup();
    paths
}

pub fn format_capacity(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "GB", "TB", "PB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1000.0 && unit < UNITS.len() - 1 {
        value /= 1000.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{value:.0} {}", UNITS[unit])
    } else {
        format!("{value:.2} {}", UNITS[unit])
    }
}

fn read_u64(path: impl AsRef<Path>) -> Option<u64> {
    fs::read_to_string(path).ok()?.trim().parse().ok()
}
fn read_trimmed(path: impl AsRef<Path>) -> Option<String> {
    let value = fs::read_to_string(path).ok()?.trim().to_string();
    (!value.is_empty()).then_some(value)
}
