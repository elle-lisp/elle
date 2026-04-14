//! Elle image plugin — raster image I/O, transforms, drawing, and analysis
//! via the `image` and `imageproc` crates.

mod analysis;
mod composite;
mod draw;
mod inspect;
mod io;
mod transform;

use std::cell::RefCell;

use image::DynamicImage;

use elle::primitives::def::PrimitiveDef;
use elle::value::fiber::{SignalBits, SIG_ERROR};
use elle::value::{error_val, Value};

// ── Type wrappers ───────────────────────────────────────────────────

/// Immutable image stored as an external value.
pub struct ImageWrap(pub DynamicImage);

/// Mutable image stored as an external value with interior mutability.
pub struct ImageMut(pub RefCell<DynamicImage>);

// ── Helpers ─────────────────────────────────────────────────────────

/// Extract a DynamicImage reference from either an immutable or mutable image.
/// For mutable images, clones the inner value (needed for transforms that
/// consume or borrow the image).
pub fn get_image(val: &Value, name: &str) -> Result<DynamicImage, (SignalBits, Value)> {
    if let Some(w) = val.as_external::<ImageWrap>() {
        Ok(w.0.clone())
    } else if let Some(m) = val.as_external::<ImageMut>() {
        Ok(m.0.borrow().clone())
    } else {
        Err((
            SIG_ERROR,
            error_val(
                "type-error",
                format!(
                    "{}: expected image or @image, got {}",
                    name,
                    val.type_name()
                ),
            ),
        ))
    }
}

/// Extract a read-only reference to an immutable image without cloning.
pub fn get_image_ref<'a>(val: &'a Value, name: &str) -> Result<ImageRef<'a>, (SignalBits, Value)> {
    if let Some(w) = val.as_external::<ImageWrap>() {
        Ok(ImageRef::Immutable(&w.0))
    } else if let Some(m) = val.as_external::<ImageMut>() {
        Ok(ImageRef::Mutable(m))
    } else {
        Err((
            SIG_ERROR,
            error_val(
                "type-error",
                format!(
                    "{}: expected image or @image, got {}",
                    name,
                    val.type_name()
                ),
            ),
        ))
    }
}

/// Reference to either image type for read-only operations.
pub enum ImageRef<'a> {
    Immutable(&'a DynamicImage),
    Mutable(&'a ImageMut),
}

impl ImageRef<'_> {
    pub fn with<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&DynamicImage) -> R,
    {
        match self {
            ImageRef::Immutable(img) => f(img),
            ImageRef::Mutable(m) => f(&m.0.borrow()),
        }
    }
}

/// Require a mutable image; error if immutable.
pub fn get_image_mut<'a>(val: &'a Value, name: &str) -> Result<&'a ImageMut, (SignalBits, Value)> {
    val.as_external::<ImageMut>().ok_or_else(|| {
        (
            SIG_ERROR,
            error_val(
                "type-error",
                format!("{}: expected @image, got {}", name, val.type_name()),
            ),
        )
    })
}

pub fn wrap_image(img: DynamicImage) -> Value {
    Value::external("image", ImageWrap(img))
}

pub fn wrap_image_mut(img: DynamicImage) -> Value {
    Value::external("@image", ImageMut(RefCell::new(img)))
}

fn require_int(val: &Value, name: &str, param: &str) -> Result<i64, (SignalBits, Value)> {
    val.as_int().ok_or_else(|| {
        (
            SIG_ERROR,
            error_val(
                "type-error",
                format!("{}: {} must be int, got {}", name, param, val.type_name()),
            ),
        )
    })
}

fn require_float(val: &Value, name: &str, param: &str) -> Result<f64, (SignalBits, Value)> {
    val.as_float()
        .or_else(|| val.as_int().map(|i| i as f64))
        .ok_or_else(|| {
            (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!(
                        "{}: {} must be number, got {}",
                        name,
                        param,
                        val.type_name()
                    ),
                ),
            )
        })
}

fn require_string(val: &Value, name: &str, param: &str) -> Result<String, (SignalBits, Value)> {
    val.with_string(|s| s.to_owned()).ok_or_else(|| {
        (
            SIG_ERROR,
            error_val(
                "type-error",
                format!(
                    "{}: {} must be string, got {}",
                    name,
                    param,
                    val.type_name()
                ),
            ),
        )
    })
}

/// Extract an RGBA color from a [r g b a] array.
pub fn extract_color(val: &Value, name: &str) -> Result<image::Rgba<u8>, (SignalBits, Value)> {
    let arr = val.as_array().ok_or_else(|| {
        (
            SIG_ERROR,
            error_val(
                "type-error",
                format!(
                    "{}: color must be [r g b a] array, got {}",
                    name,
                    val.type_name()
                ),
            ),
        )
    })?;
    if arr.len() != 4 {
        return Err((
            SIG_ERROR,
            error_val(
                "value-error",
                format!(
                    "{}: color array must have 4 elements, got {}",
                    name,
                    arr.len()
                ),
            ),
        ));
    }
    let mut rgba = [0u8; 4];
    for (i, v) in arr.iter().enumerate() {
        let n = v.as_int().ok_or_else(|| {
            (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!(
                        "{}: color component must be int, got {}",
                        name,
                        v.type_name()
                    ),
                ),
            )
        })?;
        rgba[i] = n.clamp(0, 255) as u8;
    }
    Ok(image::Rgba(rgba))
}

/// Map an image format keyword to an ImageFormat.
pub fn parse_format(val: &Value, name: &str) -> Result<image::ImageFormat, (SignalBits, Value)> {
    let kw = val.as_keyword_name().ok_or_else(|| {
        (
            SIG_ERROR,
            error_val(
                "type-error",
                format!(
                    "{}: format must be a keyword, got {}",
                    name,
                    val.type_name()
                ),
            ),
        )
    })?;
    match kw.as_str() {
        "png" => Ok(image::ImageFormat::Png),
        "jpeg" | "jpg" => Ok(image::ImageFormat::Jpeg),
        "gif" => Ok(image::ImageFormat::Gif),
        "webp" => Ok(image::ImageFormat::WebP),
        "tiff" | "tif" => Ok(image::ImageFormat::Tiff),
        "bmp" => Ok(image::ImageFormat::Bmp),
        "ico" => Ok(image::ImageFormat::Ico),
        "qoi" => Ok(image::ImageFormat::Qoi),
        _ => Err((
            SIG_ERROR,
            error_val(
                "value-error",
                format!("{}: unsupported format :{}", name, kw),
            ),
        )),
    }
}

/// Map a DynamicImage color type to a keyword name.
pub fn color_type_keyword(img: &DynamicImage) -> &'static str {
    match img {
        DynamicImage::ImageLuma8(_) => "luma8",
        DynamicImage::ImageLumaA8(_) => "lumaa8",
        DynamicImage::ImageRgb8(_) => "rgb8",
        DynamicImage::ImageRgba8(_) => "rgba8",
        DynamicImage::ImageLuma16(_) => "luma16",
        DynamicImage::ImageLumaA16(_) => "lumaa16",
        DynamicImage::ImageRgb16(_) => "rgb16",
        DynamicImage::ImageRgba16(_) => "rgba16",
        DynamicImage::ImageRgb32F(_) => "rgb32f",
        DynamicImage::ImageRgba32F(_) => "rgba32f",
        _ => "unknown",
    }
}

/// Parse a color type keyword into a constructor for blank images.
pub fn parse_color_type(kw: &str) -> Option<fn(u32, u32) -> DynamicImage> {
    match kw {
        "rgba8" => Some(DynamicImage::new_rgba8 as fn(u32, u32) -> DynamicImage),
        "rgb8" => Some(DynamicImage::new_rgb8 as fn(u32, u32) -> DynamicImage),
        "luma8" => Some(DynamicImage::new_luma8 as fn(u32, u32) -> DynamicImage),
        "lumaa8" => Some(DynamicImage::new_luma_a8 as fn(u32, u32) -> DynamicImage),
        _ => None,
    }
}

// ── Plugin init ─────────────────────────────────────────────────────

fn all_primitives() -> Vec<&'static PrimitiveDef> {
    vec![
        // I/O
        &io::READ,
        &io::WRITE,
        &io::DECODE,
        &io::ENCODE,
        // Introspection
        &inspect::WIDTH,
        &inspect::HEIGHT,
        &inspect::DIMENSIONS,
        &inspect::COLOR_TYPE,
        &inspect::PIXELS,
        &inspect::FROM_PIXELS,
        &inspect::GET_PIXEL,
        &inspect::PUT_PIXEL,
        // Mutability
        &inspect::THAW,
        &inspect::FREEZE,
        &inspect::NEW,
        // Transforms
        &transform::RESIZE,
        &transform::CROP,
        &transform::ROTATE,
        &transform::FLIP,
        // Adjustments
        &transform::BLUR,
        &transform::CONTRAST,
        &transform::BRIGHTEN,
        &transform::GRAYSCALE,
        &transform::INVERT,
        &transform::HUE_ROTATE,
        &transform::TO_RGBA8,
        &transform::TO_RGB8,
        &transform::TO_LUMA8,
        // Drawing
        &draw::DRAW_LINE,
        &draw::DRAW_RECT,
        &draw::DRAW_CIRCLE,
        &draw::FILL_RECT,
        &draw::FILL_CIRCLE,
        // Compositing
        &composite::OVERLAY,
        &composite::BLEND,
        // Analysis
        &analysis::HISTOGRAM,
        &analysis::EDGES,
        &analysis::THRESHOLD,
        &analysis::ERODE,
        &analysis::DILATE,
    ]
}

#[no_mangle]
/// # Safety
///
/// Called by Elle's plugin loader via `dlsym`.
pub unsafe extern "C" fn elle_plugin_init(ctx: &mut elle::plugin::PluginContext) -> Value {
    let prims = all_primitives();
    elle::plugin::register_and_build_refs(ctx, &prims, "image/")
}
