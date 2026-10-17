# Viper Disk Info

A native Linux disk health application written in Rust with egui. It uses CrystalDiskInfo 9.9.3 as its behavior and interface reference; the original source remains in its separate reference directory.

## Current implementation

- egui/eframe desktop interface for selecting a drive and viewing health and device details.
- Main-window layout follows the upstream drive-tab, health/temperature, device-metadata, and SMART-table arrangement.
- Linux block device enumeration through `/sys/block`.
- SMART status and common NVMe and ATA fields through `smartctl` JSON output.
- NVMe health log counters and ATA SMART attribute tables in the drive detail view.
- Fahrenheit display, green mode, serial hiding, SMART-table hiding, no-SMART drive filtering, automatic refresh intervals, raw-value formats, report copying, and temperature history graph.
- Capacity and drive metadata fallbacks from sysfs when SMART access is unavailable.

SMART access depends on `smartmontools` and permissions to access the relevant device. Values that the operating system or device does not expose are shown as unavailable. The port is in progress; vendor-specific controller support and the full CrystalDiskInfo feature set are not implemented yet.

## Build and run

Requirements: Rust (Cargo) and optionally `smartmontools`. The desktop UI uses eframe with Wayland and X11 support.

```sh
cargo run --release --bin viperdiskinfo
```

On Arch Linux, install smartmontools with the system package manager if SMART data is needed:

```sh
sudo pacman -S smartmontools
```

## Packaging

Build an x86_64 AppImage (the script downloads linuxdeploy on first use):

```sh
packaging/build-appimage.sh
```

Install or remove the system-wide binary, desktop entry, and icon:

```sh
packaging/packenger.sh install
packaging/packenger.sh uninstall
```

## Source and license

The upstream behavior reference is CrystalDiskInfo 9.9.3. The upstream CrystalDiskInfo license notice is included in [LICENSE](LICENSE). Rust dependency licenses are documented by their respective crates.

Upstream project: [Crystal Dew World](https://crystalmark.info/)
