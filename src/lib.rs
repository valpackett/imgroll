mod webp;

use snafu::{ResultExt, Snafu};
use std::{convert::TryInto, ptr, slice};

const PNG_QUANTIZE_COLORS: usize = 69;
const WEBP_QUALITY: f32 = 53.0;
const JPEG_QUALITY: f32 = 65.0;

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

    #[snafu(display("Could not encode png: {}", source))]
    PngEncode { source: lodepng::Error },

    #[snafu(display("Could not encode jpeg"))]
    JpegEncode {},

    #[snafu(display("Could not fit size value into type: {}", source))]
    ConvertInt { source: std::num::TryFromIntError },
}

pub type Result<T, E = Error> = std::result::Result<T, E>;

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct GeoLocation {
    pub longitude: f64,
    pub latitude: f64,
    pub altitude: f64,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct SrcSetEntry {
    pub src: String,
    pub width: u32,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct Source {
    pub original: bool,
    pub srcset: Vec<SrcSetEntry>,
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

pub struct OutFile {
    pub name: String,
    pub bytes: Vec<u8>,
    pub mimetype: String,
}

pub fn process_photo(file_contents: &[u8], file_name: &str) -> Result<(Photo, Vec<OutFile>)> {
    use image::GenericImageView;
    let meta = rexiv2::Metadata::new_from_buffer(&file_contents).context(MetadataParse {})?;
    let exivfmt = meta.get_media_type().context(MetadataParse {})?;
    let imag = orient_image(
        image::load_from_memory_with_format(&file_contents, format_exiv2image(&exivfmt)?).context(ImageProc {})?,
        meta.get_orientation(),
    );
    let palette = color_thief::get_palette(&imag.raw_pixels(), colortype_image2thief(imag.color())?, 10, 10)
        .context(PaletteExtract {})?;
    let (width, height) = imag.dimensions();

    let file_prefix = format!(
        "{}_{}",
        {
            use tiny_keccak::Hasher;
            let mut hasher = tiny_keccak::ParallelHash::v128(&[], 8192);
            hasher.update(&imag.raw_pixels());
            let mut buf = [0u8; 16];
            hasher.finalize(&mut buf);
            hex::encode(&buf[0..6])
        },
        slug::slugify(basename(&file_name))
    );

    let lossless = format_is_lossless(&exivfmt);

    // Always constrain the size of the main processed image
    let (imag, main_width) = if !lossless && (width > 3000 || height > 3000) {
        let i = imag.resize(3000, 3000, image::FilterType::Lanczos3);
        let w = i.width();
        (i, w)
    } else {
        (imag, width)
    };

    use rayon::prelude::*;
    let (mut source, files): (Vec<_>, Vec<_>) = encoders_for_format(&exivfmt)?
        .par_iter()
        .map(|encoder| {
            let main_result = encoder(&imag)?;
            let main_filename = format!("{}.{}.{}", file_prefix, main_width, main_result.file_ext);
            let mut files = vec![];
            files.push(OutFile {
                name: main_filename.clone(),
                bytes: main_result.bytes,
                mimetype: main_result.mime_type.to_owned(),
            });
            let mut srcset = vec![SrcSetEntry {
                src: main_filename,
                width: main_width,
            }];

            let mimetype = main_result.mime_type.to_owned();
            let mut make_thumbnail = |size| {
                let thumb = imag.resize(size, size, image::FilterType::Lanczos3);
                let result = encoder(&thumb)?;
                let filename = format!("{}.{}.{}", file_prefix, thumb.width(), result.file_ext);
                files.push(OutFile {
                    name: filename.clone(),
                    bytes: result.bytes,
                    mimetype: mimetype.clone(),
                });
                srcset.push(SrcSetEntry {
                    src: filename,
                    width: thumb.width(),
                });
                Ok(())
            };

            if !lossless && width > 2500 {
                make_thumbnail(2000)?;
            }

            if !lossless && width > 1500 {
                make_thumbnail(1000)?;
            }

            Ok((
                Source {
                    original: false,
                    srcset,
                    r#type: main_result.mime_type.to_owned(),
                },
                files,
            ))
        })
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .unzip();

    source.push(Source {
        original: true,
        srcset: vec![SrcSetEntry {
            src: file_name.to_owned(),
            width: width,
        }],
        r#type: format_exiv2mime(&exivfmt)?.to_owned(),
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
        files.into_iter().flatten().collect(),
    ))
}

fn format_exiv2image(mt: &rexiv2::MediaType) -> Result<image::ImageFormat> {
    match mt {
        rexiv2::MediaType::Jpeg => Ok(image::ImageFormat::JPEG),
        rexiv2::MediaType::Png => Ok(image::ImageFormat::PNG),
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

fn format_is_lossless(mt: &rexiv2::MediaType) -> bool {
    match mt {
        rexiv2::MediaType::Png => true,
        _f => false,
    }
}

fn encoders_for_format(mt: &rexiv2::MediaType) -> Result<&'static [Encoder]> {
    match mt {
        rexiv2::MediaType::Jpeg => Ok(&[encode_jpeg, encode_webp]),
        rexiv2::MediaType::Png => Ok(&[encode_png]),
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
    Ok(format!("data:image/webp;base64,{}", base64::encode(webp.as_slice())))
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

type Encoder = fn(&image::DynamicImage) -> Result<EncodedImg>;

struct EncodedImg {
    bytes: Vec<u8>,
    mime_type: &'static str,
    file_ext: &'static str,
}

// Big images can have less "quality": see "Compressive Images"
fn quality_bonus(imag: &image::DynamicImage) -> f32 {
    use image::GenericImageView;
    (5000.0 - f32::max(imag.width() as f32, 4900.0)) * 0.001
}

fn encode_webp(imag: &image::DynamicImage) -> Result<EncodedImg> {
    let webp =
        webp::encode(imag.clone(), webp::Quality::Lossy(WEBP_QUALITY + quality_bonus(imag))).context(WebpEncode {})?;
    let mut bytes = Vec::new();
    bytes.extend_from_slice(webp.as_slice());
    Ok(EncodedImg {
        bytes,
        mime_type: "image/webp",
        file_ext: "webp",
    })
}

fn encode_jpeg(imag: &image::DynamicImage) -> Result<EncodedImg> {
    use image::GenericImageView;
    let mut jpeg = mozjpeg::Compress::new(match imag.color() {
        image::ColorType::RGB(8) => mozjpeg::ColorSpace::JCS_RGB,
        image::ColorType::RGBA(8) => mozjpeg::ColorSpace::JCS_EXT_RGBA,
        f => return Err(Error::UnsupportedColor { format: f }),
    });
    jpeg.set_scan_optimization_mode(mozjpeg::ScanMode::AllComponentsTogether);
    jpeg.set_size(imag.width() as usize, imag.height() as usize);
    jpeg.set_quality(JPEG_QUALITY + quality_bonus(imag));
    jpeg.set_mem_dest();

    jpeg.start_compress();
    let samp = match imag.color() {
        image::ColorType::RGB(8) => imag.to_rgb().into_flat_samples(),
        image::ColorType::RGBA(8) => imag.to_rgba().into_flat_samples(),
        f => return Err(Error::UnsupportedColor { format: f }),
    };
    jpeg.write_scanlines(&samp.as_slice());
    jpeg.finish_compress();

    jpeg.data_to_vec()
        .map(|bytes| EncodedImg {
            bytes,
            mime_type: "image/jpeg",
            file_ext: "jpg",
        })
        .map_err(|_| Error::JpegEncode {})
}

fn encode_png(imag: &image::DynamicImage) -> Result<EncodedImg> {
    use exoquant::{convert_to_indexed, ditherer, optimizer, Color};
    use image::{GenericImageView, Pixel};
    let pixels = imag
        .pixels()
        .map(|(_, _, p)| {
            let cols = p.channels();
            Color::new(cols[0], cols[1], cols[2], cols[3])
        })
        .collect::<Vec<_>>();
    let width = imag.width().try_into().context(ConvertInt {})?;
    let height = imag.height().try_into().context(ConvertInt {})?;
    let (palette, indexed_pixels) = convert_to_indexed(
        &pixels,
        width,
        PNG_QUANTIZE_COLORS,
        &optimizer::KMeans,
        &ditherer::FloydSteinberg::checkered(),
    );
    let mut state = lodepng::State::new();
    unsafe {
        state.set_custom_zlib(Some(compress_zopfli), ptr::null());
    }
    for color in palette {
        let rgba = rgb::RGBA::new(color.r, color.g, color.b, color.a);
        state.info_png_mut().color.palette_add(rgba).context(PngEncode {})?;
        state.info_raw_mut().palette_add(rgba).context(PngEncode {})?;
    }
    state.info_png_mut().color.set_bitdepth(8);
    state.info_png_mut().color.colortype = lodepng::ColorType::PALETTE;
    state.info_raw_mut().set_bitdepth(8);
    state.info_raw_mut().colortype = lodepng::ColorType::PALETTE;
    let bytes = state.encode(&indexed_pixels, width, height).context(PngEncode {})?;
    Ok(EncodedImg {
        bytes,
        mime_type: "image/png",
        file_ext: "png",
    })
}

unsafe extern "C" fn compress_zopfli(
    result: &mut *mut libc::c_uchar,
    outsize: &mut usize,
    input: *const libc::c_uchar,
    insize: usize,
    _settings: *const lodepng::CompressSettings,
) -> libc::c_uint {
    // Would be nice to use a Write impl for a C buffer but whatever
    let in_slice = slice::from_raw_parts(input as *const _, insize);
    let mut bytes = Vec::new();
    if let Err(_) = zopfli::compress(&zopfli::Options::default(), &zopfli::Format::Zlib, in_slice, &mut bytes) {
        return 69;
    }
    *outsize = bytes.len();
    *result = libc::malloc(*outsize) as *mut _;
    let out_slice = slice::from_raw_parts_mut(*result, *outsize);
    out_slice.copy_from_slice(&bytes);
    0
}
