//! Bounded decoding of a loaded body picture.

use std::io::Cursor;

use ely_gpui_component::documents::HeldPicture;

pub(super) fn png(picture: &HeldPicture) -> Result<Vec<u8>, String> {
    let mut reader = image::ImageReader::new(Cursor::new(picture.bytes().as_ref()))
        .with_guessed_format()
        .map_err(|error| error.to_string())?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(16384);
    limits.max_image_height = Some(16384);
    limits.max_alloc = Some(64 * 1024 * 1024);
    reader.limits(limits);
    let image = reader.decode().map_err(|error| error.to_string())?;
    let mut png = Cursor::new(Vec::new());
    image
        .write_to(&mut png, image::ImageFormat::Png)
        .map_err(|error| error.to_string())?;
    Ok(png.into_inner())
}
