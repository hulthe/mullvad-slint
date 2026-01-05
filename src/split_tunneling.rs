use std::{
    ffi::OsStr,
    fs,
    process::{Command, Stdio},
    rc::Rc,
};

use freedesktop_desktop_entry::{DesktopEntry, IconSource};
use slint::{ComponentHandle as _, Model as _, ModelRc, Rgba8Pixel, SharedString, VecModel, Weak};

use crate::{
    RT,
    my_slint::{AppMeta, AppWindow, SplitTunneling},
};

// TODO: don't use constants, ask slint how large the icon should be
const ICON_SIZE: u16 = 128;

fn app_is_problematic(entry: &DesktopEntry) -> bool {
    entry.flatpak().is_some()
}

pub fn launch_app(app: AppMeta) {
    let result = Command::new("mullvad-exclude")
        .args(app.exec.iter())
        .stdin(Stdio::null())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn();

    match result {
        Ok(_child) => {}
        Err(e) => {
            eprintln!("Failed to spawn {}: {e}", app.title);
        }
    }
}

pub fn load_app_list(app_weak: Weak<AppWindow>) {
    RT.spawn_blocking(move || {
        let locales = &[];

        enum ImageData {
            Pixel(slint::SharedPixelBuffer<Rgba8Pixel>),
            Svg(Vec<u8>),
        }

        let mut entries: Vec<_> = freedesktop_desktop_entry::desktop_entries(locales)
            // TODO: consider processing each desktop entry in parallel
            .into_iter()
            .filter(|entry| !entry.hidden())
            .filter(|entry| !entry.no_display())
            .map(|entry| {
                let title = entry
                    .name(locales)
                    .unwrap_or(std::borrow::Cow::Borrowed(&entry.appid))
                    .to_string();
                let exec = entry
                    .parse_exec()
                    .inspect_err(|e| {
                        eprintln!("Failed to parse exec for {}: {e}", entry.appid);
                    })
                    .unwrap_or_default();
                let icon = entry
                    .icon()
                    .map(IconSource::from_unknown)
                    .and_then(|source| match source {
                        IconSource::Name(name) => {
                            freedesktop_icons::lookup(&name).with_size(ICON_SIZE).find()
                        }
                        IconSource::Path(path) => Some(path),
                    })
                    .and_then(|path| {
                        let data = fs::read(&path).ok()?;
                        if path.extension() == Some(OsStr::new("svg")) {
                            return Some(ImageData::Svg(data));
                        }

                        image::load_from_memory(&data)
                            .inspect_err(|e| {
                                eprintln!("Failed to load icon for {}: {e}", entry.appid);
                            })
                            .map(|image| {
                                let image = image
                                    // Make sure we don't load huge icons into the GUI, as that may slow it down.
                                    .resize(
                                        u32::from(ICON_SIZE),
                                        u32::from(ICON_SIZE),
                                        image::imageops::FilterType::Triangle,
                                    )
                                    .into_rgba8();

                                slint::SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(
                                    image.as_raw(),
                                    image.width(),
                                    image.height(),
                                )
                            })
                            .map(ImageData::Pixel)
                            .ok()
                    });
                let show_warning = app_is_problematic(&entry);
                (title, exec, icon, show_warning)
            })
            .collect();

        entries.sort_by(|(title_a, ..), (title_b, ..)| title_a.cmp(&title_b));

        // Copy desktop entries into the GUI.
        // As much work as possible should be done before this point
        // to avoid causing a noticable stutter in the gui.
        let _ = app_weak.upgrade_in_event_loop(move |app| {
            let app_list = entries
                .into_iter()
                .map(|(title, exec, icon, show_warning)| {
                    let icon = icon.and_then(|image| match image {
                        ImageData::Pixel(buffer) => Some(slint::Image::from_rgba8(buffer)),
                        // TODO: can svg decoding be done on another thread?
                        ImageData::Svg(buffer) => slint::Image::load_from_svg_data(&buffer).ok(),
                    });

                    let exec: VecModel<_> = exec.into_iter().map(SharedString::from).collect();
                    AppMeta {
                        title: title.into(),
                        exec: ModelRc::new(Rc::new(exec)),
                        icon: icon.unwrap_or_default(),
                        show_warning,
                    }
                })
                .collect::<VecModel<AppMeta>>();
            let app_list = ModelRc::new(Rc::new(app_list));
            app.global::<SplitTunneling>().set_app_list(app_list);
        });
    });
}
