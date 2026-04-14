//! Image transforms and adjustments.

use image::imageops::FilterType;
use image::DynamicImage;

use elle::primitives::def::PrimitiveDef;
use elle::signals::Signal;
use elle::value::fiber::{SignalBits, SIG_ERROR, SIG_OK};
use elle::value::types::Arity;
use elle::value::{error_val, Value};

use crate::{get_image, require_float, require_int, wrap_image};

// ── image/resize ────────────────────────────────────────────────────

fn parse_filter(val: &Value) -> FilterType {
    val.as_keyword_name()
        .as_deref()
        .map(|s| match s {
            "nearest" => FilterType::Nearest,
            "bilinear" | "triangle" => FilterType::Triangle,
            "catmull-rom" | "cubic" => FilterType::CatmullRom,
            "gaussian" => FilterType::Gaussian,
            "lanczos3" | "lanczos" => FilterType::Lanczos3,
            _ => FilterType::Lanczos3,
        })
        .unwrap_or(FilterType::Lanczos3)
}

fn prim_resize(args: &[Value]) -> (SignalBits, Value) {
    let img = match get_image(&args[0], "image/resize") {
        Ok(i) => i,
        Err(e) => return e,
    };
    let w = match require_int(&args[1], "image/resize", "width") {
        Ok(v) => v as u32,
        Err(e) => return e,
    };
    let h = match require_int(&args[2], "image/resize", "height") {
        Ok(v) => v as u32,
        Err(e) => return e,
    };
    let filter = if args.len() > 3 {
        parse_filter(&args[3])
    } else {
        FilterType::Lanczos3
    };
    (SIG_OK, wrap_image(img.resize_exact(w, h, filter)))
}

pub static RESIZE: PrimitiveDef = PrimitiveDef {
    name: "image/resize",
    func: prim_resize,
    signal: Signal::errors(),
    arity: Arity::Range(3, 4),
    doc: "Resize image to width x height. Optional filter: :nearest :bilinear :catmull-rom :lanczos3 (default).",
    params: &["img", "width", "height", "filter"],
    category: "image",
    example: "(image/resize img 200 150)",
    aliases: &[],
};

// ── image/crop ──────────────────────────────────────────────────────

fn prim_crop(args: &[Value]) -> (SignalBits, Value) {
    let mut img = match get_image(&args[0], "image/crop") {
        Ok(i) => i,
        Err(e) => return e,
    };
    let x = match require_int(&args[1], "image/crop", "x") {
        Ok(v) => v as u32,
        Err(e) => return e,
    };
    let y = match require_int(&args[2], "image/crop", "y") {
        Ok(v) => v as u32,
        Err(e) => return e,
    };
    let w = match require_int(&args[3], "image/crop", "width") {
        Ok(v) => v as u32,
        Err(e) => return e,
    };
    let h = match require_int(&args[4], "image/crop", "height") {
        Ok(v) => v as u32,
        Err(e) => return e,
    };
    (SIG_OK, wrap_image(img.crop(x, y, w, h)))
}

pub static CROP: PrimitiveDef = PrimitiveDef {
    name: "image/crop",
    func: prim_crop,
    signal: Signal::errors(),
    arity: Arity::Exact(5),
    doc: "Crop a region from an image. Returns new image.",
    params: &["img", "x", "y", "width", "height"],
    category: "image",
    example: "(image/crop img 10 10 100 100)",
    aliases: &[],
};

// ── image/rotate ────────────────────────────────────────────────────

fn prim_rotate(args: &[Value]) -> (SignalBits, Value) {
    let img = match get_image(&args[0], "image/rotate") {
        Ok(i) => i,
        Err(e) => return e,
    };
    let angle = match args[1].as_keyword_name() {
        Some(s) => s,
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!(
                        "image/rotate: angle must be :r90 :r180 :r270, got {}",
                        args[1].type_name()
                    ),
                ),
            )
        }
    };
    let result = match angle.as_str() {
        "r90" | "90" => img.rotate90(),
        "r180" | "180" => img.rotate180(),
        "r270" | "270" => img.rotate270(),
        _ => {
            return (
                SIG_ERROR,
                error_val(
                    "value-error",
                    format!("image/rotate: expected :r90 :r180 :r270, got :{}", angle),
                ),
            )
        }
    };
    (SIG_OK, wrap_image(result))
}

pub static ROTATE: PrimitiveDef = PrimitiveDef {
    name: "image/rotate",
    func: prim_rotate,
    signal: Signal::errors(),
    arity: Arity::Exact(2),
    doc: "Rotate image by :r90, :r180, or :r270.",
    params: &["img", "angle"],
    category: "image",
    example: "(image/rotate img :r90)",
    aliases: &[],
};

// ── image/flip ──────────────────────────────────────────────────────

fn prim_flip(args: &[Value]) -> (SignalBits, Value) {
    let img = match get_image(&args[0], "image/flip") {
        Ok(i) => i,
        Err(e) => return e,
    };
    let dir = match args[1].as_keyword_name() {
        Some(s) => s,
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!(
                        "image/flip: direction must be :h or :v, got {}",
                        args[1].type_name()
                    ),
                ),
            )
        }
    };
    let result = match dir.as_str() {
        "h" | "horizontal" => img.fliph(),
        "v" | "vertical" => img.flipv(),
        _ => {
            return (
                SIG_ERROR,
                error_val(
                    "value-error",
                    format!("image/flip: expected :h or :v, got :{}", dir),
                ),
            )
        }
    };
    (SIG_OK, wrap_image(result))
}

pub static FLIP: PrimitiveDef = PrimitiveDef {
    name: "image/flip",
    func: prim_flip,
    signal: Signal::errors(),
    arity: Arity::Exact(2),
    doc: "Flip image :h (horizontal) or :v (vertical).",
    params: &["img", "direction"],
    category: "image",
    example: "(image/flip img :h)",
    aliases: &[],
};

// ── Adjustments ─────────────────────────────────────────────────────

fn prim_blur(args: &[Value]) -> (SignalBits, Value) {
    let img = match get_image(&args[0], "image/blur") {
        Ok(i) => i,
        Err(e) => return e,
    };
    let sigma = match require_float(&args[1], "image/blur", "sigma") {
        Ok(v) => v as f32,
        Err(e) => return e,
    };
    (SIG_OK, wrap_image(img.blur(sigma)))
}

pub static BLUR: PrimitiveDef = PrimitiveDef {
    name: "image/blur",
    func: prim_blur,
    signal: Signal::errors(),
    arity: Arity::Exact(2),
    doc: "Apply Gaussian blur with given sigma.",
    params: &["img", "sigma"],
    category: "image",
    example: "(image/blur img 2.0)",
    aliases: &[],
};

fn prim_contrast(args: &[Value]) -> (SignalBits, Value) {
    let img = match get_image(&args[0], "image/contrast") {
        Ok(i) => i,
        Err(e) => return e,
    };
    let c = match require_float(&args[1], "image/contrast", "contrast") {
        Ok(v) => v as f32,
        Err(e) => return e,
    };
    (SIG_OK, wrap_image(img.adjust_contrast(c)))
}

pub static CONTRAST: PrimitiveDef = PrimitiveDef {
    name: "image/contrast",
    func: prim_contrast,
    signal: Signal::errors(),
    arity: Arity::Exact(2),
    doc: "Adjust contrast. Positive values increase, negative decrease.",
    params: &["img", "contrast"],
    category: "image",
    example: "(image/contrast img 20.0)",
    aliases: &[],
};

fn prim_brighten(args: &[Value]) -> (SignalBits, Value) {
    let img = match get_image(&args[0], "image/brighten") {
        Ok(i) => i,
        Err(e) => return e,
    };
    let b = match require_int(&args[1], "image/brighten", "brightness") {
        Ok(v) => v as i32,
        Err(e) => return e,
    };
    (SIG_OK, wrap_image(img.brighten(b)))
}

pub static BRIGHTEN: PrimitiveDef = PrimitiveDef {
    name: "image/brighten",
    func: prim_brighten,
    signal: Signal::errors(),
    arity: Arity::Exact(2),
    doc: "Adjust brightness. Positive brightens, negative darkens.",
    params: &["img", "amount"],
    category: "image",
    example: "(image/brighten img 30)",
    aliases: &[],
};

fn prim_grayscale(args: &[Value]) -> (SignalBits, Value) {
    let img = match get_image(&args[0], "image/grayscale") {
        Ok(i) => i,
        Err(e) => return e,
    };
    (SIG_OK, wrap_image(img.grayscale()))
}

pub static GRAYSCALE: PrimitiveDef = PrimitiveDef {
    name: "image/grayscale",
    func: prim_grayscale,
    signal: Signal::errors(),
    arity: Arity::Exact(1),
    doc: "Convert image to grayscale.",
    params: &["img"],
    category: "image",
    example: "(image/grayscale img)",
    aliases: &[],
};

fn prim_invert(args: &[Value]) -> (SignalBits, Value) {
    let mut img = match get_image(&args[0], "image/invert") {
        Ok(i) => i,
        Err(e) => return e,
    };
    img.invert();
    (SIG_OK, wrap_image(img))
}

pub static INVERT: PrimitiveDef = PrimitiveDef {
    name: "image/invert",
    func: prim_invert,
    signal: Signal::errors(),
    arity: Arity::Exact(1),
    doc: "Invert all pixel colors.",
    params: &["img"],
    category: "image",
    example: "(image/invert img)",
    aliases: &[],
};

fn prim_hue_rotate(args: &[Value]) -> (SignalBits, Value) {
    let img = match get_image(&args[0], "image/hue-rotate") {
        Ok(i) => i,
        Err(e) => return e,
    };
    let deg = match require_int(&args[1], "image/hue-rotate", "degrees") {
        Ok(v) => v as i32,
        Err(e) => return e,
    };
    (SIG_OK, wrap_image(img.huerotate(deg)))
}

pub static HUE_ROTATE: PrimitiveDef = PrimitiveDef {
    name: "image/hue-rotate",
    func: prim_hue_rotate,
    signal: Signal::errors(),
    arity: Arity::Exact(2),
    doc: "Rotate hue by given degrees.",
    params: &["img", "degrees"],
    category: "image",
    example: "(image/hue-rotate img 90)",
    aliases: &[],
};

// ── Color format conversion ─────────────────────────────────────────

fn prim_to_rgba8(args: &[Value]) -> (SignalBits, Value) {
    let img = match get_image(&args[0], "image/to-rgba8") {
        Ok(i) => i,
        Err(e) => return e,
    };
    (SIG_OK, wrap_image(DynamicImage::ImageRgba8(img.to_rgba8())))
}

pub static TO_RGBA8: PrimitiveDef = PrimitiveDef {
    name: "image/to-rgba8",
    func: prim_to_rgba8,
    signal: Signal::errors(),
    arity: Arity::Exact(1),
    doc: "Convert image to RGBA8 color type.",
    params: &["img"],
    category: "image",
    example: "(image/to-rgba8 img)",
    aliases: &[],
};

fn prim_to_rgb8(args: &[Value]) -> (SignalBits, Value) {
    let img = match get_image(&args[0], "image/to-rgb8") {
        Ok(i) => i,
        Err(e) => return e,
    };
    (SIG_OK, wrap_image(DynamicImage::ImageRgb8(img.to_rgb8())))
}

pub static TO_RGB8: PrimitiveDef = PrimitiveDef {
    name: "image/to-rgb8",
    func: prim_to_rgb8,
    signal: Signal::errors(),
    arity: Arity::Exact(1),
    doc: "Convert image to RGB8 color type.",
    params: &["img"],
    category: "image",
    example: "(image/to-rgb8 img)",
    aliases: &[],
};

fn prim_to_luma8(args: &[Value]) -> (SignalBits, Value) {
    let img = match get_image(&args[0], "image/to-luma8") {
        Ok(i) => i,
        Err(e) => return e,
    };
    (SIG_OK, wrap_image(DynamicImage::ImageLuma8(img.to_luma8())))
}

pub static TO_LUMA8: PrimitiveDef = PrimitiveDef {
    name: "image/to-luma8",
    func: prim_to_luma8,
    signal: Signal::errors(),
    arity: Arity::Exact(1),
    doc: "Convert image to 8-bit grayscale.",
    params: &["img"],
    category: "image",
    example: "(image/to-luma8 img)",
    aliases: &[],
};
