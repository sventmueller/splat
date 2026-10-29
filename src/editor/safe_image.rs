//! Untrusted image data, decoded in Rust. The image parsers of macOS have been the target of zero-click exploits,
//! so data from the web reaches them only as raw pixels or as a PDF written here.

use std::sync::Arc;

use svg2pdf::usvg;

/// Larger downloads and SVG sources are not shown.
pub const MAX_BYTES: usize = 10 << 20;
/// Larger image files are not shown; camera photos stay well below.
pub const MAX_FILE_BYTES: usize = 64 << 20;
/// Pixels an SVG filter may be rasterised to.
const MAX_PIXELS: f64 = 16_000_000.0;
/// Larger images are not decoded at all.
const MAX_SIDE: u32 = 16_384;
/// Longest side of a decoded image; larger ones are scaled down, which stays sharp at any preview width.
pub const SHOWN_SIDE: u32 = 4096;

pub enum Decoded {
    Pdf(Vec<u8>),
    Rgba { width: u32, height: u32, pixels: Vec<u8> },
}

/// SVG becomes a PDF; PNG, JPEG, GIF, WebP and BMP become pixels; anything else is refused.
pub fn decode(bytes: &[u8], fonts: Arc<usvg::fontdb::Database>) -> Option<Decoded> {
    if bytes.len() > MAX_FILE_BYTES {
        return None;
    }
    if let Some(svg) = std::str::from_utf8(bytes).ok().filter(|t| t.contains("<svg")) {
        return svg_to_pdf(svg, fonts).map(Decoded::Pdf);
    }
    let mut reader = image::ImageReader::new(std::io::Cursor::new(bytes))
        .with_guessed_format()
        .ok()?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(MAX_SIDE);
    limits.max_image_height = Some(MAX_SIDE);
    limits.max_alloc = Some(512 << 20);
    reader.limits(limits);
    let mut decoder = reader.into_decoder().ok()?;
    // Cameras store photos unrotated and note the orientation in the EXIF data.
    let orientation =
        image::ImageDecoder::orientation(&mut decoder).unwrap_or(image::metadata::Orientation::NoTransforms);
    let mut image = image::DynamicImage::from_decoder(decoder).ok()?;
    image.apply_orientation(orientation);
    if image.width().max(image.height()) > SHOWN_SIDE {
        image = image.resize(SHOWN_SIDE, SHOWN_SIDE, image::imageops::FilterType::Triangle);
    }
    let image = image.into_rgba8();
    Some(Decoded::Rgba {
        width: image.width(),
        height: image.height(),
        pixels: image.into_raw(),
    })
}

/// Converts an SVG without reading any file it references; embedded images may only be PNG or SVG, which are
/// decoded in Rust, while JPEG would pass through to the PDF and reach the system decoder.
pub fn svg_to_pdf(svg: &str, fonts: Arc<usvg::fontdb::Database>) -> Option<Vec<u8>> {
    if svg.len() > MAX_BYTES {
        return None;
    }
    let tree = usvg::Tree::from_str(svg, &options(fonts)).ok()?;
    let raster_scale = raster_scale(&tree)?;
    let conversion = svg2pdf::ConversionOptions {
        raster_scale,
        ..Default::default()
    };
    svg2pdf::to_pdf(&tree, conversion, svg2pdf::PageOptions::default()).ok()
}

fn options(fonts: Arc<usvg::fontdb::Database>) -> usvg::Options<'static> {
    usvg::Options {
        fontdb: fonts,
        image_href_resolver: usvg::ImageHrefResolver {
            resolve_data: Box::new(|mime, data, options| match mime {
                "image/png" => Some(usvg::ImageKind::PNG(data)),
                "image/svg+xml" => (usvg::ImageHrefResolver::default_data_resolver())(mime, data, options),
                _ => None,
            }),
            resolve_string: Box::new(|_, _| None),
        },
        ..usvg::Options::default()
    }
}

/// svg2pdf rasterises filters over their whole region; a tiny SVG can declare a region of gigapixels. The scale is
/// lowered until the largest region fits, and an SVG that would need less than a twentieth is refused.
fn raster_scale(tree: &usvg::Tree) -> Option<f32> {
    fn largest(group: &usvg::Group, max: &mut f64) {
        let t = group.abs_transform();
        let area_scale = f64::from((t.sx * t.sy - t.kx * t.ky).abs()).max(1.0);
        for filter in group.filters() {
            let r = filter.rect();
            *max = max.max(f64::from(r.width()) * f64::from(r.height()) * area_scale);
        }
        for node in group.children() {
            if let usvg::Node::Group(g) = node {
                largest(g, max);
            }
            node.subroots(|g| largest(g, max));
        }
    }
    const DEFAULT: f64 = 1.5;
    let mut max = 0.0;
    largest(tree.root(), &mut max);
    let scale = if max > 0.0 {
        (MAX_PIXELS / max).sqrt().min(DEFAULT)
    } else {
        DEFAULT
    };
    (scale >= 0.05).then_some(scale as f32)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fonts() -> Arc<usvg::fontdb::Database> {
        Arc::new(usvg::fontdb::Database::new())
    }

    #[test]
    fn svg_never_reads_local_files() {
        let dir = std::env::temp_dir().join(format!("splat-svg-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let png = dir.join("secret.png");
        let mut pixels = Vec::new();
        image::RgbaImage::new(2, 2)
            .write_to(&mut std::io::Cursor::new(&mut pixels), image::ImageFormat::Png)
            .unwrap();
        std::fs::write(&png, &pixels).unwrap();
        let svg = format!(
            r#"<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" width="10" height="10"><image width="10" height="10" xlink:href="{}"/><image width="10" height="10" href="file://{}"/></svg>"#,
            png.display(),
            png.display()
        );
        let tree = usvg::Tree::from_str(&svg, &options(fonts())).unwrap();
        assert!(!tree.root().has_children(), "a local image was read");
        assert!(svg_to_pdf(&svg, fonts()).is_some());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn filter_bombs_stay_small() {
        let bomb = r#"<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10"><filter id="f" filterUnits="userSpaceOnUse" x="0" y="0" width="4000" height="4000"><feGaussianBlur stdDeviation="50"/></filter><rect width="10" height="10" filter="url(#f)"/></svg>"#;
        let started = std::time::Instant::now();
        assert!(svg_to_pdf(bomb, fonts()).is_some());
        assert!(
            started.elapsed() < std::time::Duration::from_secs(8),
            "{:?}",
            started.elapsed()
        );
        let huge = bomb.replace("4000", "4000000");
        assert!(svg_to_pdf(&huge, fonts()).is_none());
    }

    #[test]
    fn raster_images_decode_with_limits() {
        let mut png = Vec::new();
        image::RgbaImage::new(3, 2)
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap();
        assert!(matches!(
            decode(&png, fonts()),
            Some(Decoded::Rgba {
                width: 3,
                height: 2,
                ..
            })
        ));
        assert!(decode(b"%PDF-1.7 not an image", fonts()).is_none());
        let mut wide = Vec::new();
        image::RgbaImage::new(5000, 10)
            .write_to(&mut std::io::Cursor::new(&mut wide), image::ImageFormat::Png)
            .unwrap();
        assert!(matches!(
            decode(&wide, fonts()),
            Some(Decoded::Rgba {
                width: SHOWN_SIDE,
                height: 8,
                ..
            })
        ));
        // A PNG header claiming 100000 x 100000 pixels.
        let mut big = png.clone();
        big[16..20].copy_from_slice(&100_000u32.to_be_bytes());
        big[20..24].copy_from_slice(&100_000u32.to_be_bytes());
        assert!(decode(&big, fonts()).is_none());
    }
}
