mod webp;

use snafu::{ResultExt, Snafu};
use std::{collections::HashMap, ptr, slice};

#[derive(Debug, Snafu)]
pub enum Error {
    #[snafu(display("Unable to process image: {}", source))]
    ImageProc { source: image::ImageError },

    #[snafu(display("Unsupported color format: {:?}", format))]
    UnsupportedColor { format: image::ColorType },

    #[snafu(display("Unable to extract palette: {}", source))]
    PaletteExtract { source: color_thief::Error },

    #[snafu(display("Unable to parse metadata: {}", source))]
    MetadataParse { source: rexiv2::Rexiv2Error },

    #[snafu(display("Unsupported file format: {}", format))]
    UnsupportedFormat { format: rexiv2::MediaType },

    #[snafu(display("Could not encode webp: {}", source))]
    WebpEncode { source: webp::Error },
}

pub type Result<T, E = Error> = std::result::Result<T, E>;

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct GeoLocation {
    pub longitude: f64,
    pub latitude: f64,
    pub altitude: f64,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct Source {
    pub original: bool,
    pub src: String,
    pub r#type: String,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct Photo {
    pub tiny_preview: String,
    pub source: Vec<Source>,
    pub height: u32,
    pub width: u32,
    pub palette: Vec<rgb::RGB8>,
    pub geo: Option<GeoLocation>,
    pub aperture: Option<f64>,
    pub shutter_speed: Option<num_rational::Ratio<i32>>,
    pub focal_length: Option<f64>,
    pub iso: Option<i32>,
}

pub fn process_photo(file_contents: &[u8]) -> Result<Photo> {
    use image::GenericImageView;
    let meta = rexiv2::Metadata::new_from_buffer(&file_contents).context(MetadataParse {})?;
    let imag = orient_image(
        image::load_from_memory_with_format(
            &file_contents,
            format_exiv2image(meta.get_media_type().context(MetadataParse {})?)?,
        )
        .context(ImageProc {})?,
        meta.get_orientation(),
    );
    let palette = color_thief::get_palette(
        &imag.raw_pixels(),
        colortype_image2thief(imag.color())?,
        10,
        10,
    )
    .context(PaletteExtract {})?;
    let (width, height) = imag.dimensions();

    let mut source = Vec::new();

    Ok(Photo {
        tiny_preview: make_tiny_preview(&imag)?,
        source,
        width,
        height,
        palette,
        geo: meta.get_gps_info().map(
            |rexiv2::GpsInfo {
                 latitude,
                 longitude,
                 altitude,
             }| GeoLocation {
                latitude,
                longitude,
                altitude,
            },
        ),
        aperture: meta.get_fnumber(),
        shutter_speed: meta.get_exposure_time(),
        focal_length: meta.get_focal_length(),
        iso: meta.get_iso_speed(),
    })
}

fn format_exiv2image(mt: rexiv2::MediaType) -> Result<image::ImageFormat> {
    match mt {
        rexiv2::MediaType::Jpeg => Ok(image::ImageFormat::JPEG),
        rexiv2::MediaType::Png => Ok(image::ImageFormat::PNG),
        f => Err(Error::UnsupportedFormat { format: f }),
    }
}

fn orient_image(imag: image::DynamicImage, ori: rexiv2::Orientation) -> image::DynamicImage {
    use rexiv2::Orientation::*;
    match ori {
        HorizontalFlip => imag.fliph(),
        Rotate180 => imag.rotate180(),
        VerticalFlip => imag.flipv(),
        Rotate90HorizontalFlip => imag.rotate90().fliph(),
        Rotate90 => imag.rotate90(),
        Rotate90VerticalFlip => imag.rotate90().flipv(),
        Rotate270 => imag.rotate270(),
        _ => imag,
    }
}

fn colortype_image2thief(t: image::ColorType) -> Result<color_thief::ColorFormat> {
    match t {
        image::ColorType::RGB(8) => Ok(color_thief::ColorFormat::Rgb),
        image::ColorType::RGBA(8) => Ok(color_thief::ColorFormat::Rgba),
        f => Err(Error::UnsupportedColor { format: f }),
    }
}

pub fn make_tiny_preview(imag: &image::DynamicImage) -> Result<String> {
    let thumb = imag.resize(48, 48, image::FilterType::Gaussian);
    let webp = webp::encode(thumb, webp::Quality::Lossy(0.2)).context(WebpEncode {})?;
    Ok(format!(
        "data:image/webp;base64,{}",
        base64::encode(webp.as_slice())
    ))
}
