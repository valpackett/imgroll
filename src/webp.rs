use og_libwebp_sys::{WebPEncodeLosslessRGB, WebPEncodeLosslessRGBA, WebPEncodeRGB, WebPEncodeRGBA, WebPFree};
use snafu::{ResultExt, Snafu};
use std::{convert::TryInto, ptr, slice};

#[derive(Debug, Snafu)]
pub enum Error {
    #[snafu(display("Unsupported color format: {:?}", format))]
    UnsupportedColor { format: image::ColorType },

    #[snafu(display("Could not fit size value into signed type: {}", source))]
    ConvertSigned { source: std::num::TryFromIntError },

    #[snafu(display("Could not encode: {}", ret))]
    Encode { ret: usize },
}

pub type Result<T, E = Error> = std::result::Result<T, E>;

pub struct WebPOinter {
    ptr: *mut u8,
    cnt: usize,
}

impl WebPOinter {
    pub fn as_slice(&self) -> &[u8] {
        unsafe { slice::from_raw_parts(self.ptr as *const u8, self.cnt) }
    }
}

impl Drop for WebPOinter {
    fn drop(&mut self) {
        unsafe {
            WebPFree(self.ptr as *mut _);
        }
    }
}

pub enum Quality {
    Lossless,
    Lossy(f32),
}

pub fn encode(imag: image::DynamicImage, quality: Quality) -> Result<WebPOinter> {
    use image::GenericImageView;
    use Quality::*;
    let samp = match imag.color() {
        image::ColorType::Rgb8 => imag.to_rgb8().into_flat_samples(),
        image::ColorType::Rgba8 => imag.to_rgba8().into_flat_samples(),
        f => return Err(Error::UnsupportedColor { format: f }),
    };
    let (width, height) = imag.dimensions();
    let (_, _, rowstride) = samp.strides_cwh();
    let mut result = WebPOinter {
        ptr: ptr::null_mut(),
        cnt: 0,
    };
    let w = width.try_into().context(ConvertSigned {})?;
    let h = height.try_into().context(ConvertSigned {})?;
    let s = rowstride.try_into().context(ConvertSigned {})?;
    let ret = unsafe {
        match (imag.color(), quality) {
            (image::ColorType::Rgb8, Lossy(q)) => WebPEncodeRGB(&samp.as_slice()[0], w, h, s, q, &mut result.ptr),
            (image::ColorType::Rgba8, Lossy(q)) => WebPEncodeRGBA(&samp.as_slice()[0], w, h, s, q, &mut result.ptr),
            (image::ColorType::Rgb8, Lossless) => {
                WebPEncodeLosslessRGB(&samp.as_slice()[0], w, h, s, &mut result.ptr)
            },
            (image::ColorType::Rgba8, Lossless) => {
                WebPEncodeLosslessRGBA(&samp.as_slice()[0], w, h, s, &mut result.ptr)
            },
            (f, _) => return Err(Error::UnsupportedColor { format: f }),
        }
    };
    if ret < 1 || result.ptr == ptr::null_mut() {
        return Err(Error::Encode { ret });
    }
    result.cnt = ret;
    Ok(result)
}
