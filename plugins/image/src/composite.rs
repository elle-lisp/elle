//! Image compositing: overlay and blend.

use image::DynamicImage;

use elle::primitives::def::PrimitiveDef;
use elle::signals::Signal;
use elle::value::fiber::{SignalBits, SIG_ERROR, SIG_OK};
use elle::value::types::Arity;
use elle::value::{error_val, Value};

use crate::{get_image, require_float, require_int, wrap_image};

// ── image/overlay ───────────────────────────────────────────────────

fn prim_overlay(args: &[Value]) -> (SignalBits, Value) {
    let mut base = match get_image(&args[0], "image/overlay") {
        Ok(i) => i.to_rgba8(),
        Err(e) => return e,
    };
    let overlay = match get_image(&args[1], "image/overlay") {
        Ok(i) => i.to_rgba8(),
        Err(e) => return e,
    };
    let x = match require_int(&args[2], "image/overlay", "x") {
        Ok(v) => v,
        Err(e) => return e,
    };
    let y = match require_int(&args[3], "image/overlay", "y") {
        Ok(v) => v,
        Err(e) => return e,
    };
    image::imageops::overlay(&mut base, &overlay, x, y);
    (SIG_OK, wrap_image(DynamicImage::ImageRgba8(base)))
}

pub static OVERLAY: PrimitiveDef = PrimitiveDef {
    name: "image/overlay",
    func: prim_overlay,
    signal: Signal::errors(),
    arity: Arity::Exact(4),
    doc: "Composite overlay image onto base at position (x, y). Alpha-blended.",
    params: &["base", "overlay", "x", "y"],
    category: "image",
    example: "(image/overlay base-img overlay-img 10 20)",
    aliases: &[],
};

// ── image/blend ─────────────────────────────────────────────────────

fn prim_blend(args: &[Value]) -> (SignalBits, Value) {
    let img1 = match get_image(&args[0], "image/blend") {
        Ok(i) => i.to_rgba8(),
        Err(e) => return e,
    };
    let img2 = match get_image(&args[1], "image/blend") {
        Ok(i) => i.to_rgba8(),
        Err(e) => return e,
    };
    let alpha = match require_float(&args[2], "image/blend", "alpha") {
        Ok(v) => v as f32,
        Err(e) => return e,
    };
    if img1.dimensions() != img2.dimensions() {
        return (
            SIG_ERROR,
            error_val(
                "value-error",
                format!(
                    "image/blend: images must have same dimensions, got {}x{} and {}x{}",
                    img1.width(),
                    img1.height(),
                    img2.width(),
                    img2.height()
                ),
            ),
        );
    }
    let alpha = alpha.clamp(0.0, 1.0);
    let inv = 1.0 - alpha;
    let mut out = image::RgbaImage::new(img1.width(), img1.height());
    for (x, y, px1) in img1.enumerate_pixels() {
        let px2 = img2.get_pixel(x, y);
        let blended = image::Rgba([
            (px1[0] as f32 * inv + px2[0] as f32 * alpha) as u8,
            (px1[1] as f32 * inv + px2[1] as f32 * alpha) as u8,
            (px1[2] as f32 * inv + px2[2] as f32 * alpha) as u8,
            (px1[3] as f32 * inv + px2[3] as f32 * alpha) as u8,
        ]);
        out.put_pixel(x, y, blended);
    }
    (SIG_OK, wrap_image(DynamicImage::ImageRgba8(out)))
}

pub static BLEND: PrimitiveDef = PrimitiveDef {
    name: "image/blend",
    func: prim_blend,
    signal: Signal::errors(),
    arity: Arity::Exact(3),
    doc: "Alpha-blend two same-size images. alpha=0 is all img1, alpha=1 is all img2.",
    params: &["img1", "img2", "alpha"],
    category: "image",
    example: "(image/blend img1 img2 0.5)",
    aliases: &[],
};
