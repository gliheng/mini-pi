use std::sync::Arc;

use gpui::{Image, ImageFormat, ImageSource};
use image::{ImageBuffer, ImageEncoder, Luma};
use qrcode::QrCode;

/// Generate a QR code for `url` and return it as a GPUI image source.
pub fn qr_image_source(url: &str) -> Option<ImageSource> {
    let code = match QrCode::new(url.as_bytes()) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("[remote] failed to create QR code: {}", e);
            return None;
        }
    };
    let image: ImageBuffer<Luma<u8>, Vec<u8>> = code
        .render::<Luma<u8>>()
        .module_dimensions(4, 4)
        .build();

    let mut bytes = Vec::new();
    {
        let encoder = image::codecs::png::PngEncoder::new(&mut bytes);
        if let Err(e) = encoder.write_image(
            image.as_raw(),
            image.width(),
            image.height(),
            image::ExtendedColorType::L8,
        ) {
            eprintln!("[remote] failed to encode QR code PNG: {}", e);
            return None;
        }
    }

    Some(ImageSource::Image(Arc::new(Image::from_bytes(
        ImageFormat::Png,
        bytes,
    ))))
}

