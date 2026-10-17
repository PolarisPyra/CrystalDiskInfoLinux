# CrystalDiskInfo for Linux

A native Linux and Rust port of CrystalDiskInfo. This project retains the supplied CrystalDiskInfo 9.9.3 source snapshot for reference while the Linux implementation is being ported into `src/`.

## Current implementation

- GTK4 desktop interface for selecting a drive and viewing health and device details.
- Linux block device enumeration through `/sys/block`.
- SMART status and common NVMe and ATA fields through `smartctl` JSON output.
- Capacity and drive metadata fallbacks from sysfs when SMART access is unavailable.

SMART access depends on `smartmontools` and permissions to access the relevant device. Values that the operating system or device does not expose are shown as unavailable. The port is in progress; vendor-specific controller support and the full CrystalDiskInfo feature set are not implemented yet.

## Build and run

Requirements: Rust (Cargo), GTK4 development libraries, and optionally `smartmontools`.

```sh
cargo run --release
```

On Arch Linux, install GTK4 and smartmontools with the system package manager:

```sh
sudo pacman -S gtk4 smartmontools
```

## Source and license

The included upstream source snapshot is CrystalDiskInfo 9.9.3. The project is distributed under the MIT license; see [License/COPYRIGHT.txt](License/COPYRIGHT.txt) and the third-party notices in `License/`.

Upstream project: [Crystal Dew World](https://crystalmark.info/)
