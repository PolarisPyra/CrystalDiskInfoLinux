# CrystalDiskInfo for Linux

A native Linux and Rust port of CrystalDiskInfo. This project retains the supplied CrystalDiskInfo 9.9.3 source snapshot for reference while the Linux implementation is being ported into `src/`.

## Current implementation

- egui/eframe desktop interface for selecting a drive and viewing health and device details.
- Linux block device enumeration through `/sys/block`.
- SMART status and common NVMe and ATA fields through `smartctl` JSON output.
- NVMe health log counters and ATA SMART attribute tables in the drive detail view.
- Capacity and drive metadata fallbacks from sysfs when SMART access is unavailable.

SMART access depends on `smartmontools` and permissions to access the relevant device. Values that the operating system or device does not expose are shown as unavailable. The port is in progress; vendor-specific controller support and the full CrystalDiskInfo feature set are not implemented yet.

## Build and run

Requirements: Rust (Cargo) and optionally `smartmontools`. The desktop UI uses eframe with Wayland and X11 support.

```sh
cargo run --release
```

On Arch Linux, install smartmontools with the system package manager if SMART data is needed:

```sh
sudo pacman -S smartmontools
```

## Source and license

The included upstream source snapshot is CrystalDiskInfo 9.9.3. The project is distributed under the MIT license; see [License/COPYRIGHT.txt](License/COPYRIGHT.txt) and the third-party notices in `License/`.

Upstream project: [Crystal Dew World](https://crystalmark.info/)
