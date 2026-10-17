use crate::storage::{self, Drive};
use gtk::{
    Align, Application, ApplicationWindow, Box as GtkBox, Button, Grid, Label, ListBox, ListBoxRow,
    Orientation, ScrolledWindow, SelectionMode, glib, prelude::*,
};
use std::{cell::RefCell, rc::Rc, sync::mpsc, thread, time::Duration};

pub fn build(app: &Application) {
    let window = ApplicationWindow::builder()
        .application(app)
        .title("CrystalDiskInfo for Linux")
        .default_width(1040)
        .default_height(660)
        .build();

    let root = GtkBox::new(Orientation::Vertical, 0);
    root.add_css_class("window-root");
    root.append(&header());

    let content = GtkBox::new(Orientation::Horizontal, 0);
    let sidebar = GtkBox::new(Orientation::Vertical, 12);
    sidebar.set_width_request(310);
    sidebar.set_margin_top(24);
    sidebar.set_margin_bottom(24);
    sidebar.set_margin_start(20);
    sidebar.set_margin_end(20);
    sidebar.append(&section_title("STORAGE DEVICES"));
    let list = ListBox::new();
    list.set_selection_mode(SelectionMode::Single);
    list.add_css_class("navigation-sidebar");
    sidebar.append(&list);
    content.append(&sidebar);

    let detail_scroll = ScrolledWindow::new();
    detail_scroll.set_hexpand(true);
    detail_scroll.set_vexpand(true);
    let detail = GtkBox::new(Orientation::Vertical, 18);
    detail.set_hexpand(true);
    detail.set_margin_top(30);
    detail.set_margin_bottom(30);
    detail.set_margin_start(30);
    detail.set_margin_end(30);
    detail.append(&section_title("DRIVE HEALTH"));
    let health_card = GtkBox::new(Orientation::Vertical, 8);
    health_card.add_css_class("health-card");
    let health = Label::new(Some("Select a drive"));
    health.set_halign(Align::Start);
    health.add_css_class("health-value");
    let health_note = Label::new(Some("Disk health and SMART information appear here."));
    health_note.set_halign(Align::Start);
    health_note.add_css_class("dim-label");
    health_card.append(&health);
    health_card.append(&health_note);
    detail.append(&health_card);
    detail.append(&section_title("DEVICE DETAILS"));
    let details = GtkBox::new(Orientation::Vertical, 0);
    details.add_css_class("details-card");
    detail.append(&details);
    let nvme_title = section_title("NVME HEALTH LOG");
    detail.append(&nvme_title);
    let nvme_metrics = Grid::new();
    nvme_metrics.set_column_spacing(24);
    nvme_metrics.set_row_spacing(8);
    nvme_metrics.add_css_class("details-card");
    detail.append(&nvme_metrics);
    let ata_title = section_title("ATA S.M.A.R.T. ATTRIBUTES");
    detail.append(&ata_title);
    let ata_attributes = Grid::new();
    ata_attributes.set_column_spacing(18);
    ata_attributes.set_row_spacing(8);
    ata_attributes.add_css_class("details-card");
    detail.append(&ata_attributes);
    let footer = Label::new(Some(
        "SMART access may require elevated device permissions or smartmontools.",
    ));
    footer.set_halign(Align::Start);
    footer.add_css_class("dim-label");
    detail.append(&footer);
    detail_scroll.set_child(Some(&detail));
    content.append(&detail_scroll);
    root.append(&content);

    let drives = Rc::new(RefCell::new(Vec::<Drive>::new()));
    let refresh = Button::with_label("Refresh");
    refresh.add_css_class("flat");
    let list_for_refresh = list.clone();
    let drives_for_refresh = drives.clone();
    let health_for_refresh = health.clone();
    let note_for_refresh = health_note.clone();
    let details_for_refresh = details.clone();
    let nvme_for_refresh = nvme_metrics.clone();
    let ata_for_refresh = ata_attributes.clone();
    let nvme_title_for_refresh = nvme_title.clone();
    let ata_title_for_refresh = ata_title.clone();
    let refresh_for_callback = refresh.clone();
    refresh.connect_clicked(move |_| {
        refresh_drives(
            &list_for_refresh,
            &drives_for_refresh,
            &health_for_refresh,
            &note_for_refresh,
            &details_for_refresh,
            &nvme_for_refresh,
            &ata_for_refresh,
            &nvme_title_for_refresh,
            &ata_title_for_refresh,
            &refresh_for_callback,
        )
    });
    if let Some(header_bar) = root.first_child().and_then(|w| w.downcast::<GtkBox>().ok()) {
        if let Some(button) = header_bar.last_child() {
            button.set_visible(false);
        }
        header_bar.append(&refresh);
    }

    let drives_for_select = drives.clone();
    let health_for_select = health.clone();
    let note_for_select = health_note.clone();
    let details_for_select = details.clone();
    let nvme_for_select = nvme_metrics.clone();
    let ata_for_select = ata_attributes.clone();
    let nvme_title_for_select = nvme_title.clone();
    let ata_title_for_select = ata_title.clone();
    list.connect_row_selected(move |_, row| {
        if let Some(row) = row {
            let index = row.index() as usize;
            if let Some(drive) = drives_for_select.borrow().get(index) {
                show_drive(
                    drive,
                    &health_for_select,
                    &note_for_select,
                    &details_for_select,
                    &nvme_for_select,
                    &ata_for_select,
                    &nvme_title_for_select,
                    &ata_title_for_select,
                );
            }
        }
    });
    window.set_child(Some(&root));
    refresh_drives(
        &list,
        &drives,
        &health,
        &health_note,
        &details,
        &nvme_metrics,
        &ata_attributes,
        &nvme_title,
        &ata_title,
        &refresh,
    );
    window.present();
}

fn header() -> GtkBox {
    let bar = GtkBox::new(Orientation::Horizontal, 12);
    bar.set_margin_top(16);
    bar.set_margin_bottom(16);
    bar.set_margin_start(20);
    bar.set_margin_end(20);
    bar.add_css_class("header-bar");
    let title_box = GtkBox::new(Orientation::Vertical, 2);
    let title = Label::new(Some("CrystalDiskInfo"));
    title.set_halign(Align::Start);
    title.add_css_class("app-title");
    let subtitle = Label::new(Some("Storage health monitor"));
    subtitle.set_halign(Align::Start);
    subtitle.add_css_class("dim-label");
    title_box.append(&title);
    title_box.append(&subtitle);
    bar.append(&title_box);
    bar.set_hexpand(true);
    bar
}

fn section_title(text: &str) -> Label {
    let label = Label::new(Some(text));
    label.set_halign(Align::Start);
    label.add_css_class("section-title");
    label
}

fn refresh_drives(
    list: &ListBox,
    drives: &Rc<RefCell<Vec<Drive>>>,
    health: &Label,
    note: &Label,
    details: &GtkBox,
    nvme_metrics: &Grid,
    ata_attributes: &Grid,
    nvme_title: &Label,
    ata_title: &Label,
    refresh_button: &Button,
) {
    refresh_button.set_sensitive(false);
    health.set_text("Scanning drives...");
    note.set_text("Reading device metadata and SMART data.");
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        let _ = sender.send(storage::discover());
    });

    let list = list.clone();
    let drives = drives.clone();
    let health = health.clone();
    let note = note.clone();
    let details = details.clone();
    let nvme_metrics = nvme_metrics.clone();
    let ata_attributes = ata_attributes.clone();
    let nvme_title = nvme_title.clone();
    let ata_title = ata_title.clone();
    let refresh_button = refresh_button.clone();
    glib::timeout_add_local(Duration::from_millis(50), move || {
        match receiver.try_recv() {
            Ok(found) => {
                populate_drives(
                    &list,
                    &drives,
                    &health,
                    &note,
                    &details,
                    &nvme_metrics,
                    &ata_attributes,
                    &nvme_title,
                    &ata_title,
                    found,
                );
                refresh_button.set_sensitive(true);
                glib::ControlFlow::Break
            }
            Err(mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
            Err(mpsc::TryRecvError::Disconnected) => {
                health.set_text("Drive scan failed");
                note.set_text("The background device scan ended unexpectedly.");
                refresh_button.set_sensitive(true);
                glib::ControlFlow::Break
            }
        }
    });
}

#[allow(clippy::too_many_arguments)]
fn populate_drives(
    list: &ListBox,
    drives: &Rc<RefCell<Vec<Drive>>>,
    health: &Label,
    note: &Label,
    details: &GtkBox,
    nvme_metrics: &Grid,
    ata_attributes: &Grid,
    nvme_title: &Label,
    ata_title: &Label,
    found: Vec<Drive>,
) {
    while let Some(child) = list.first_child() {
        list.remove(&child);
    }
    *drives.borrow_mut() = found;
    let borrowed = drives.borrow();
    for drive in borrowed.iter() {
        let row = ListBoxRow::new();
        let box_ = GtkBox::new(Orientation::Vertical, 5);
        box_.set_margin_top(10);
        box_.set_margin_bottom(10);
        box_.set_margin_start(12);
        box_.set_margin_end(12);
        let model = Label::new(Some(&drive.model));
        model.set_halign(Align::Start);
        model.add_css_class("device-name");
        let info = Label::new(Some(&format!(
            "{}  ·  {}",
            drive.name,
            storage::format_capacity(drive.capacity)
        )));
        info.set_halign(Align::Start);
        info.add_css_class("dim-label");
        box_.append(&model);
        box_.append(&info);
        row.set_child(Some(&box_));
        list.append(&row);
    }
    if borrowed.is_empty() {
        health.set_text("No drives found");
        note.set_text("Linux block devices were not available under /sys/block.");
        clear_details(details);
        clear_grid(nvme_metrics);
        clear_grid(ata_attributes);
        nvme_title.set_visible(false);
        ata_title.set_visible(false);
    } else {
        list.select_row(list.row_at_index(0).as_ref());
    }
}

fn show_drive(
    drive: &Drive,
    health: &Label,
    note: &Label,
    details: &GtkBox,
    nvme_metrics: &Grid,
    ata_attributes: &Grid,
    nvme_title: &Label,
    ata_title: &Label,
) {
    health.set_text(if drive.assessment.is_empty() {
        "Health unavailable"
    } else {
        &drive.assessment
    });
    health.remove_css_class("health-good");
    health.remove_css_class("health-warning");
    match drive.smart_status {
        Some(true) => health.add_css_class("health-good"),
        Some(false) => health.add_css_class("health-warning"),
        None => (),
    }
    note.set_text(drive.note.as_deref().unwrap_or(if drive.smart_available {
        "Status reported by smartctl. Attribute interpretation is evolving."
    } else {
        "Install smartmontools and grant device access to read SMART data."
    }));
    clear_details(details);
    add_detail(details, "Device", &format!("/dev/{}", drive.name));
    add_detail(details, "Model", &drive.model);
    add_detail(
        details,
        "Serial number",
        if drive.serial.is_empty() {
            "Unavailable"
        } else {
            &drive.serial
        },
    );
    add_detail(details, "Drive type", &drive.kind);
    add_detail(
        details,
        "Capacity",
        &storage::format_capacity(drive.capacity),
    );
    add_detail(
        details,
        "Temperature",
        &drive
            .temperature_c
            .map(|v| format!("{v} °C"))
            .unwrap_or_else(|| "Unavailable".into()),
    );
    add_detail(
        details,
        "Power-on hours",
        &drive
            .power_on_hours
            .map(|v| format!("{v} hours"))
            .unwrap_or_else(|| "Unavailable".into()),
    );
    add_detail(
        details,
        "Estimated NVMe life remaining",
        &drive
            .health_percent
            .map(|v| format!("{v}%"))
            .unwrap_or_else(|| "Unavailable".into()),
    );
    populate_nvme_metrics(nvme_metrics, drive);
    populate_ata_attributes(ata_attributes, drive);
    nvme_title.set_visible(!drive.nvme_metrics.is_empty());
    ata_title.set_visible(!drive.ata_attributes.is_empty());
}

fn clear_details(details: &GtkBox) {
    while let Some(child) = details.first_child() {
        details.remove(&child);
    }
}

fn add_detail(details: &GtkBox, name: &str, value: &str) {
    let row = GtkBox::new(Orientation::Horizontal, 12);
    row.set_margin_top(10);
    row.set_margin_bottom(10);
    row.set_margin_start(14);
    row.set_margin_end(14);
    let key = Label::new(Some(name));
    key.set_halign(Align::Start);
    key.set_hexpand(true);
    key.add_css_class("dim-label");
    let value = Label::new(Some(value));
    value.set_halign(Align::End);
    value.set_selectable(true);
    value.add_css_class("detail-value");
    row.append(&key);
    row.append(&value);
    details.append(&row);
    let separator = gtk::Separator::new(Orientation::Horizontal);
    details.append(&separator);
}

fn clear_grid(grid: &Grid) {
    while let Some(child) = grid.first_child() {
        grid.remove(&child);
    }
}

fn grid_label(text: &str, column: i32, row: i32, grid: &Grid, bold: bool) {
    let label = Label::new(Some(text));
    label.set_halign(if column == 0 {
        Align::Start
    } else {
        Align::End
    });
    label.set_selectable(true);
    label.add_css_class(if bold {
        "table-heading"
    } else {
        "detail-value"
    });
    grid.attach(&label, column, row, 1, 1);
}

fn populate_nvme_metrics(grid: &Grid, drive: &Drive) {
    clear_grid(grid);
    grid_label("Metric", 0, 0, grid, true);
    grid_label("Value", 1, 0, grid, true);
    for (index, metric) in drive.nvme_metrics.iter().enumerate() {
        let row = index as i32 + 1;
        grid_label(&metric.name, 0, row, grid, false);
        grid_label(&metric.value, 1, row, grid, false);
    }
    grid.set_visible(!drive.nvme_metrics.is_empty());
}

fn populate_ata_attributes(grid: &Grid, drive: &Drive) {
    clear_grid(grid);
    let headers = [
        "ID",
        "Attribute",
        "Current",
        "Worst",
        "Threshold",
        "Raw value",
    ];
    for (column, header) in headers.iter().enumerate() {
        grid_label(header, column as i32, 0, grid, true);
    }
    for (index, attribute) in drive.ata_attributes.iter().enumerate() {
        let row = index as i32 + 1;
        let id = attribute.id.to_string();
        let current = attribute.current.to_string();
        let worst = attribute.worst.to_string();
        let threshold = attribute.threshold.to_string();
        let name = if attribute.failed {
            format!("{} (failed)", attribute.name)
        } else {
            attribute.name.clone()
        };
        for (column, value) in [
            id.as_str(),
            name.as_str(),
            current.as_str(),
            worst.as_str(),
            threshold.as_str(),
            attribute.raw.as_str(),
        ]
        .iter()
        .enumerate()
        {
            grid_label(value, column as i32, row, grid, false);
        }
    }
    grid.set_visible(!drive.ata_attributes.is_empty());
}

pub fn install_css() {
    let provider = gtk::CssProvider::new();
    provider.load_from_data(".header-bar { background: @headerbar_bg_color; border-bottom: 1px solid alpha(@borders, .65); } .app-title { font-size: 18px; font-weight: 700; } .section-title { font-size: 11px; font-weight: 700; letter-spacing: 1px; opacity: .68; } .health-card, .details-card { padding: 18px; border-radius: 12px; background: alpha(@theme_fg_color, .045); } .health-value { font-size: 30px; font-weight: 700; } .health-good { color: #2b9b62; } .health-warning { color: #d34f45; } .device-name, .detail-value { font-weight: 600; }");
    if let Some(display) = gtk::gdk::Display::default() {
        gtk::style_context_add_provider_for_display(
            &display,
            &provider,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    }
    glib::set_application_name("CrystalDiskInfo");
}
