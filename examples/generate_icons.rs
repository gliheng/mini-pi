use std::fs::{self, File};
use std::io::{BufReader, BufWriter, Cursor, Write};
use std::path::PathBuf;

fn render_png(svg: &str, size: u32) -> Vec<u8> {
    let tree = resvg::usvg::Tree::from_str(svg, &resvg::usvg::Options::default())
        .expect("failed to parse SVG");
    let mut pixmap = resvg::tiny_skia::Pixmap::new(size, size).expect("failed to allocate pixmap");
    pixmap.fill(resvg::tiny_skia::Color::TRANSPARENT);

    let scale_x = size as f32 / tree.size().width() as f32;
    let scale_y = size as f32 / tree.size().height() as f32;
    let transform = resvg::tiny_skia::Transform::from_scale(scale_x, scale_y);

    resvg::render(&tree, transform, &mut pixmap.as_mut());
    pixmap.encode_png().expect("failed to encode PNG")
}

fn write_ico(sizes: &[u32], svg: &str, out: PathBuf) {
    let mut icon_dir = ico::IconDir::new(ico::ResourceType::Icon);
    for &size in sizes {
        let png = render_png(svg, size);
        let rgba = image::load_from_memory(&png)
            .expect("failed to decode rendered PNG")
            .to_rgba8();
        let icon_image = ico::IconImage::from_rgba_data(size, size, rgba.into_raw());
        let entry = ico::IconDirEntry::encode(&icon_image).expect("failed to encode icon entry");
        icon_dir.add_entry(entry);
    }
    let file = File::create(&out).expect("failed to create ico file");
    let mut writer = BufWriter::new(file);
    icon_dir
        .write(&mut writer)
        .expect("failed to write ico file");
    writer.flush().unwrap();
    println!("wrote {}", out.display());
}

fn write_icns(sizes: &[u32], svg: &str, out: PathBuf) {
    let mut family = icns::IconFamily::new();
    for &size in sizes {
        let png = render_png(svg, size);
        let image = icns::Image::read_png(BufReader::new(Cursor::new(png)))
            .expect("failed to decode rendered PNG");
        family.add_icon(&image).expect("failed to add icon");
    }
    let file = File::create(&out).expect("failed to create icns file");
    let mut writer = BufWriter::new(file);
    family
        .write(&mut writer)
        .expect("failed to write icns file");
    writer.flush().unwrap();
    println!("wrote {}", out.display());
}

fn main() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let logo_svg = fs::read_to_string(root.join("assets").join("logo.svg"))
        .expect("failed to read assets/logo.svg");

    write_ico(
        &[16, 32, 48, 64, 128, 256],
        &logo_svg,
        root.join("assets").join("icon.ico"),
    );
    write_icns(
        &[16, 32, 64, 128, 256, 512, 1024],
        &logo_svg,
        root.join("assets").join("icon.icns"),
    );
}
