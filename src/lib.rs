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

pub fn process_photo(
    file_contents: &[u8],
    file_name: &str,
) -> Result<(Photo, HashMap<String, Vec<u8>>)> {
    use image::GenericImageView;
    let meta = rexiv2::Metadata::new_from_buffer(&file_contents).context(MetadataParse {})?;
    let exivfmt = meta.get_media_type().context(MetadataParse {})?;
    let imag = orient_image(
        image::load_from_memory_with_format(&file_contents, format_exiv2image(&exivfmt)?)
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

    let mut files = HashMap::new();
    let mut source = Vec::new();

    let file_prefix = format!(
        "{}_{}",
        hex::encode(&tiny_keccak::shake128(&imag.raw_pixels())[0..6]),
        slug::slugify(basename(&file_name))
    );

    source.push(Source {
        original: true,
        src: file_name.to_owned(),
        r#type: format_exiv2mime(&exivfmt)?.to_owned(),
    });

    let webp_file = format!("{}.webp", file_prefix);
    let webp = webp::encode(imag.clone(), webp::Quality::Lossy(0.6)).context(WebpEncode {})?;
    files.insert(webp_file.clone(), {
        let mut v = Vec::new();
        v.extend_from_slice(webp.as_slice());
        v
    });
    source.push(Source {
        original: false,
        src: webp_file,
        r#type: "image/webp".to_owned(),
    });


    Ok((
        Photo {
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
        },
        files,
    ))
}

fn format_exiv2image(mt: &rexiv2::MediaType) -> Result<image::ImageFormat> {
    match mt {
        rexiv2::MediaType::Jpeg => Ok(image::ImageFormat::JPEG),
        rexiv2::MediaType::Png => Ok(image::ImageFormat::PNG),
        f => Err(Error::UnsupportedFormat { format: f.clone() }),
    }
}

fn format_exiv2ext(mt: &rexiv2::MediaType) -> Result<&'static str> {
    match mt {
        rexiv2::MediaType::Jpeg => Ok("jpg"),
        rexiv2::MediaType::Png => Ok("png"),
        f => Err(Error::UnsupportedFormat { format: f.clone() }),
    }
}

fn format_exiv2mime(mt: &rexiv2::MediaType) -> Result<&'static str> {
    match mt {
        rexiv2::MediaType::Jpeg => Ok("image/jpeg"),
        rexiv2::MediaType::Png => Ok("image/png"),
        f => Err(Error::UnsupportedFormat { format: f.clone() }),
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

fn basename(path: &str) -> String {
    let mut pieces = path.rsplit('/');
    let mut parts = match pieces.next() {
        Some(p) => p,
        None => path,
    }
    .split('.');
    match parts.next() {
        Some(p) => p.into(),
        None => path.into(),
    }
}

fn encode_webp(imag: &image::DynamicImage, prefix: &str) -> Result<(Source, Vec<u8>)> {
    let name = format!("{}.webp", prefix);
    let webp = webp::encode(imag.clone(), webp::Quality::Lossy(0.6)).context(WebpEncode {})?;
}
