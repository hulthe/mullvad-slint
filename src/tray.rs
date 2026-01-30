use image::{GenericImageView, ImageFormat};
use tray_item::{IconSource, TrayItem};

pub fn create_tray_icon() -> anyhow::Result<TrayItem> {
    // load image and convert to the correct format
    let tray_icon_image = image::load_from_memory_with_format(
        include_bytes!("../tray/lock-1.png"),
        ImageFormat::Png,
    )?;
    let (width, height) = tray_icon_image.dimensions();
    let tray_icon_image = tray_icon_image
        .into_rgba8()
        .into_vec()
        .chunks_exact(4)
        .flat_map(|rgba| {
            let &[r, g, b, a] = rgba else { unreachable!() };
            [a, r, g, b]
        })
        .collect::<Vec<u8>>();

    let tray_icon_image = IconSource::Data {
        data: tray_icon_image,
        width: width as i32,
        height: height as i32,
    };

    let tray = TrayItem::new("Mullvad VPN (Slint)", tray_icon_image)?;

    // TODO: sync icon with connection state
    // tray.set_icon(icon)

    Ok(tray)
}
