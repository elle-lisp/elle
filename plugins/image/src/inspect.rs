//! Image introspection, pixel access, and mutability conversion.

use image::{DynamicImage, GenericImage, GenericImageView};

use elle::primitives::def::PrimitiveDef;
use elle::signals::Signal;
use elle::value::fiber::{SignalBits, SIG_ERROR, SIG_OK};
use elle::value::types::Arity;
use elle::value::{error_val, Value};

use crate::{
    color_type_keyword, extract_color, get_image, get_image_mut, get_image_ref, parse_color_type,
    require_int, wrap_image, wrap_image_mut,
};

// ── image/width ─────────────────────────────────────────────────────

fn prim_width(args: &[Value]) -> (SignalBits, Value) {
    let r = match get_image_ref(&args[0], "image/width") {
        Ok(r) => r,
        Err(e) => return e,
    };
    (SIG_OK, Value::int(r.with(|img| img.width() as i64)))
}

pub static WIDTH: PrimitiveDef = PrimitiveDef {
    name: "image/width",
    func: prim_width,
    signal: Signal::silent(),
    arity: Arity::Exact(1),
    doc: "Return the width of an image in pixels.",
    params: &["img"],
    category: "image",
    example: "(image/width img)",
    aliases: &[],
};

// ── image/height ────────────────────────────────────────────────────

fn prim_height(args: &[Value]) -> (SignalBits, Value) {
    let r = match get_image_ref(&args[0], "image/height") {
        Ok(r) => r,
        Err(e) => return e,
    };
    (SIG_OK, Value::int(r.with(|img| img.height() as i64)))
}

pub static HEIGHT: PrimitiveDef = PrimitiveDef {
    name: "image/height",
    func: prim_height,
    signal: Signal::silent(),
    arity: Arity::Exact(1),
    doc: "Return the height of an image in pixels.",
    params: &["img"],
    category: "image",
    example: "(image/height img)",
    aliases: &[],
};

// ── image/dimensions ────────────────────────────────────────────────

fn prim_dimensions(args: &[Value]) -> (SignalBits, Value) {
    let r = match get_image_ref(&args[0], "image/dimensions") {
        Ok(r) => r,
        Err(e) => return e,
    };
    let (w, h) = r.with(|img| img.dimensions());
    (
        SIG_OK,
        Value::array(vec![Value::int(w as i64), Value::int(h as i64)]),
    )
}

pub static DIMENSIONS: PrimitiveDef = PrimitiveDef {
    name: "image/dimensions",
    func: prim_dimensions,
    signal: Signal::silent(),
    arity: Arity::Exact(1),
    doc: "Return [width height] of an image.",
    params: &["img"],
    category: "image",
    example: "(image/dimensions img)",
    aliases: &[],
};

// ── image/color-type ────────────────────────────────────────────────

fn prim_color_type(args: &[Value]) -> (SignalBits, Value) {
    let r = match get_image_ref(&args[0], "image/color-type") {
        Ok(r) => r,
        Err(e) => return e,
    };
    (SIG_OK, Value::keyword(r.with(color_type_keyword)))
}

pub static COLOR_TYPE: PrimitiveDef = PrimitiveDef {
    name: "image/color-type",
    func: prim_color_type,
    signal: Signal::silent(),
    arity: Arity::Exact(1),
    doc: "Return the color type keyword of an image (:rgba8 :rgb8 :luma8 etc.).",
    params: &["img"],
    category: "image",
    example: "(image/color-type img)",
    aliases: &[],
};

// ── image/pixels ────────────────────────────────────────────────────

fn prim_pixels(args: &[Value]) -> (SignalBits, Value) {
    let r = match get_image_ref(&args[0], "image/pixels") {
        Ok(r) => r,
        Err(e) => return e,
    };
    (SIG_OK, Value::bytes(r.with(|img| img.as_bytes().to_vec())))
}

pub static PIXELS: PrimitiveDef = PrimitiveDef {
    name: "image/pixels",
    func: prim_pixels,
    signal: Signal::silent(),
    arity: Arity::Exact(1),
    doc: "Return the raw pixel data as bytes.",
    params: &["img"],
    category: "image",
    example: "(image/pixels img)",
    aliases: &[],
};

// ── image/from-pixels ───────────────────────────────────────────────

fn prim_from_pixels(args: &[Value]) -> (SignalBits, Value) {
    let w = match require_int(&args[0], "image/from-pixels", "width") {
        Ok(v) => v as u32,
        Err(e) => return e,
    };
    let h = match require_int(&args[1], "image/from-pixels", "height") {
        Ok(v) => v as u32,
        Err(e) => return e,
    };
    let fmt_kw = match args[2].as_keyword_name() {
        Some(s) => s,
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!(
                        "image/from-pixels: format must be keyword, got {}",
                        args[2].type_name()
                    ),
                ),
            )
        }
    };
    let data = if let Some(b) = args[3].as_bytes() {
        b.to_vec()
    } else if let Some(bm) = args[3].as_bytes_mut() {
        bm.borrow().clone()
    } else {
        return (
            SIG_ERROR,
            error_val(
                "type-error",
                format!(
                    "image/from-pixels: data must be bytes, got {}",
                    args[3].type_name()
                ),
            ),
        );
    };
    let result = match fmt_kw.as_str() {
        "rgba8" => image::RgbaImage::from_raw(w, h, data).map(DynamicImage::ImageRgba8),
        "rgb8" => image::RgbImage::from_raw(w, h, data).map(DynamicImage::ImageRgb8),
        "luma8" => image::GrayImage::from_raw(w, h, data).map(DynamicImage::ImageLuma8),
        _ => {
            return (
                SIG_ERROR,
                error_val(
                    "value-error",
                    format!("image/from-pixels: unsupported format :{}", fmt_kw),
                ),
            )
        }
    };
    match result {
        Some(img) => (SIG_OK, wrap_image(img)),
        None => (
            SIG_ERROR,
            error_val(
                "value-error",
                format!(
                    "image/from-pixels: data length {} does not match {}x{} :{}",
                    args[3].as_bytes().map(|b| b.len()).unwrap_or(0),
                    w,
                    h,
                    fmt_kw
                ),
            ),
        ),
    }
}

pub static FROM_PIXELS: PrimitiveDef = PrimitiveDef {
    name: "image/from-pixels",
    func: prim_from_pixels,
    signal: Signal::errors(),
    arity: Arity::Exact(4),
    doc: "Construct an immutable image from raw pixel data. Format: :rgba8 :rgb8 :luma8.",
    params: &["width", "height", "format", "bytes"],
    category: "image",
    example: "(image/from-pixels 2 2 :rgba8 pixel-bytes)",
    aliases: &[],
};

// ── image/get-pixel ─────────────────────────────────────────────────

fn prim_get_pixel(args: &[Value]) -> (SignalBits, Value) {
    let r = match get_image_ref(&args[0], "image/get-pixel") {
        Ok(r) => r,
        Err(e) => return e,
    };
    let x = match require_int(&args[1], "image/get-pixel", "x") {
        Ok(v) => v as u32,
        Err(e) => return e,
    };
    let y = match require_int(&args[2], "image/get-pixel", "y") {
        Ok(v) => v as u32,
        Err(e) => return e,
    };
    r.with(|img| {
        if x >= img.width() || y >= img.height() {
            (
                SIG_ERROR,
                error_val(
                    "range-error",
                    format!(
                        "image/get-pixel: ({}, {}) out of bounds for {}x{} image",
                        x,
                        y,
                        img.width(),
                        img.height()
                    ),
                ),
            )
        } else {
            let px = img.to_rgba8().get_pixel(x, y).0;
            (
                SIG_OK,
                Value::array(vec![
                    Value::int(px[0] as i64),
                    Value::int(px[1] as i64),
                    Value::int(px[2] as i64),
                    Value::int(px[3] as i64),
                ]),
            )
        }
    })
}

pub static GET_PIXEL: PrimitiveDef = PrimitiveDef {
    name: "image/get-pixel",
    func: prim_get_pixel,
    signal: Signal::errors(),
    arity: Arity::Exact(3),
    doc: "Get pixel at (x, y) as [r g b a] array (0-255).",
    params: &["img", "x", "y"],
    category: "image",
    example: "(image/get-pixel img 0 0)",
    aliases: &[],
};

// ── image/put-pixel ─────────────────────────────────────────────────

fn prim_put_pixel(args: &[Value]) -> (SignalBits, Value) {
    let x = match require_int(&args[1], "image/put-pixel", "x") {
        Ok(v) => v as u32,
        Err(e) => return e,
    };
    let y = match require_int(&args[2], "image/put-pixel", "y") {
        Ok(v) => v as u32,
        Err(e) => return e,
    };
    let color = match extract_color(&args[3], "image/put-pixel") {
        Ok(c) => c,
        Err(e) => return e,
    };

    // Mutable path: mutate in place
    if let Some(m) = args[0].as_external::<crate::ImageMut>() {
        let mut img = m.0.borrow_mut();
        if x >= img.width() || y >= img.height() {
            return (
                SIG_ERROR,
                error_val(
                    "range-error",
                    format!(
                        "image/put-pixel: ({}, {}) out of bounds for {}x{} image",
                        x,
                        y,
                        img.width(),
                        img.height()
                    ),
                ),
            );
        }
        img.as_mut_rgba8()
            .map(|buf| buf.put_pixel(x, y, color))
            .or_else(|| {
                img.as_mut_rgb8().map(|buf| {
                    buf.put_pixel(x, y, image::Rgb([color.0[0], color.0[1], color.0[2]]))
                })
            })
            .or_else(|| {
                img.as_mut_luma8()
                    .map(|buf| buf.put_pixel(x, y, image::Luma([color.0[0]])))
            });
        return (SIG_OK, Value::NIL);
    }

    // Immutable path: clone and modify
    let mut img = match get_image(&args[0], "image/put-pixel") {
        Ok(i) => i,
        Err(e) => return e,
    };
    if x >= img.width() || y >= img.height() {
        return (
            SIG_ERROR,
            error_val(
                "range-error",
                format!(
                    "image/put-pixel: ({}, {}) out of bounds for {}x{} image",
                    x,
                    y,
                    img.width(),
                    img.height()
                ),
            ),
        );
    }
    img.put_pixel(x, y, color);
    (SIG_OK, wrap_image(img))
}

pub static PUT_PIXEL: PrimitiveDef = PrimitiveDef {
    name: "image/put-pixel",
    func: prim_put_pixel,
    signal: Signal::errors(),
    arity: Arity::Exact(4),
    doc: "Set pixel at (x, y) to [r g b a]. On immutable image returns new image; on @image mutates in place.",
    params: &["img", "x", "y", "color"],
    category: "image",
    example: "(image/put-pixel img 0 0 [255 0 0 255])",
    aliases: &[],
};

// ── image/thaw ──────────────────────────────────────────────────────

fn prim_thaw(args: &[Value]) -> (SignalBits, Value) {
    let img = match get_image(&args[0], "image/thaw") {
        Ok(i) => i,
        Err(e) => return e,
    };
    (SIG_OK, wrap_image_mut(img))
}

pub static THAW: PrimitiveDef = PrimitiveDef {
    name: "image/thaw",
    func: prim_thaw,
    signal: Signal::errors(),
    arity: Arity::Exact(1),
    doc: "Convert an immutable image to a mutable @image (copy).",
    params: &["img"],
    category: "image",
    example: "(image/thaw img)",
    aliases: &[],
};

// ── image/freeze ────────────────────────────────────────────────────

fn prim_freeze(args: &[Value]) -> (SignalBits, Value) {
    let m = match get_image_mut(&args[0], "image/freeze") {
        Ok(m) => m,
        Err(e) => return e,
    };
    (SIG_OK, wrap_image(m.0.borrow().clone()))
}

pub static FREEZE: PrimitiveDef = PrimitiveDef {
    name: "image/freeze",
    func: prim_freeze,
    signal: Signal::errors(),
    arity: Arity::Exact(1),
    doc: "Convert a mutable @image to an immutable image (snapshot).",
    params: &["img"],
    category: "image",
    example: "(image/freeze @img)",
    aliases: &[],
};

// ── image/new ───────────────────────────────────────────────────────

fn prim_new(args: &[Value]) -> (SignalBits, Value) {
    let w = match require_int(&args[0], "image/new", "width") {
        Ok(v) => v as u32,
        Err(e) => return e,
    };
    let h = match require_int(&args[1], "image/new", "height") {
        Ok(v) => v as u32,
        Err(e) => return e,
    };
    let fmt_kw = match args[2].as_keyword_name() {
        Some(s) => s,
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!(
                        "image/new: format must be keyword, got {}",
                        args[2].type_name()
                    ),
                ),
            )
        }
    };
    match parse_color_type(&fmt_kw) {
        Some(ctor) => (SIG_OK, wrap_image_mut(ctor(w, h))),
        None => (
            SIG_ERROR,
            error_val(
                "value-error",
                format!("image/new: unsupported format :{}", fmt_kw),
            ),
        ),
    }
}

pub static NEW: PrimitiveDef = PrimitiveDef {
    name: "image/new",
    func: prim_new,
    signal: Signal::errors(),
    arity: Arity::Exact(3),
    doc: "Create a blank mutable @image. Format: :rgba8 :rgb8 :luma8 :lumaa8.",
    params: &["width", "height", "format"],
    category: "image",
    example: "(image/new 100 100 :rgba8)",
    aliases: &[],
};
