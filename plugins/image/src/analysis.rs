//! Image analysis: histogram, edge detection, threshold, morphology.

use std::collections::BTreeMap;

use image::DynamicImage;
use imageproc::contrast;
use imageproc::edges;
use imageproc::morphology;

use elle::primitives::def::PrimitiveDef;
use elle::signals::Signal;
use elle::value::fiber::{SignalBits, SIG_ERROR, SIG_OK};
use elle::value::types::{Arity, TableKey};
use elle::value::{error_val, Value};

use crate::{get_image, require_int, wrap_image};

// ── image/histogram ─────────────────────────────────────────────────

fn prim_histogram(args: &[Value]) -> (SignalBits, Value) {
    let img = match get_image(&args[0], "image/histogram") {
        Ok(i) => i,
        Err(e) => return e,
    };
    let rgba = img.to_rgba8();
    let mut r_hist = vec![0i64; 256];
    let mut g_hist = vec![0i64; 256];
    let mut b_hist = vec![0i64; 256];
    let mut a_hist = vec![0i64; 256];

    for px in rgba.pixels() {
        r_hist[px[0] as usize] += 1;
        g_hist[px[1] as usize] += 1;
        b_hist[px[2] as usize] += 1;
        a_hist[px[3] as usize] += 1;
    }

    let to_array = |h: Vec<i64>| Value::array(h.into_iter().map(Value::int).collect());

    let mut fields = BTreeMap::new();
    fields.insert(TableKey::Keyword("r".into()), to_array(r_hist));
    fields.insert(TableKey::Keyword("g".into()), to_array(g_hist));
    fields.insert(TableKey::Keyword("b".into()), to_array(b_hist));
    fields.insert(TableKey::Keyword("a".into()), to_array(a_hist));

    (SIG_OK, Value::struct_from(fields))
}

pub static HISTOGRAM: PrimitiveDef = PrimitiveDef {
    name: "image/histogram",
    func: prim_histogram,
    signal: Signal::errors(),
    arity: Arity::Exact(1),
    doc: "Compute per-channel histograms. Returns {:r :g :b :a} with 256-element arrays.",
    params: &["img"],
    category: "image",
    example: "(image/histogram img)",
    aliases: &[],
};

// ── image/edges ─────────────────────────────────────────────────────

fn prim_edges(args: &[Value]) -> (SignalBits, Value) {
    let img = match get_image(&args[0], "image/edges") {
        Ok(i) => i,
        Err(e) => return e,
    };
    let algo = if args.len() > 1 {
        args[1]
            .as_keyword_name()
            .unwrap_or_else(|| "canny".to_string())
    } else {
        "canny".to_string()
    };
    let gray = img.to_luma8();
    let result = match algo.as_str() {
        "canny" => edges::canny(&gray, 50.0, 100.0),
        "sobel" => {
            // Sobel via horizontal + vertical gradient magnitude
            let h = imageproc::gradients::horizontal_sobel(&gray);
            let v = imageproc::gradients::vertical_sobel(&gray);
            let mut out = image::GrayImage::new(gray.width(), gray.height());
            for (x, y, px) in out.enumerate_pixels_mut() {
                let hv = h.get_pixel(x, y)[0] as f64;
                let vv = v.get_pixel(x, y)[0] as f64;
                let mag = (hv * hv + vv * vv).sqrt().min(255.0) as u8;
                *px = image::Luma([mag]);
            }
            out
        }
        _ => {
            return (
                SIG_ERROR,
                error_val(
                    "value-error",
                    format!(
                        "image/edges: unknown algorithm :{}, expected :canny or :sobel",
                        algo
                    ),
                ),
            )
        }
    };
    (SIG_OK, wrap_image(DynamicImage::ImageLuma8(result)))
}

pub static EDGES: PrimitiveDef = PrimitiveDef {
    name: "image/edges",
    func: prim_edges,
    signal: Signal::errors(),
    arity: Arity::Range(1, 2),
    doc: "Detect edges. Optional algorithm: :canny (default) or :sobel. Returns grayscale image.",
    params: &["img", "algo"],
    category: "image",
    example: "(image/edges img :canny)",
    aliases: &[],
};

// ── image/threshold ─────────────────────────────────────────────────

fn prim_threshold(args: &[Value]) -> (SignalBits, Value) {
    let img = match get_image(&args[0], "image/threshold") {
        Ok(i) => i,
        Err(e) => return e,
    };
    let t = match require_int(&args[1], "image/threshold", "threshold") {
        Ok(v) => v.clamp(0, 255) as u8,
        Err(e) => return e,
    };
    let gray = img.to_luma8();
    let result = contrast::threshold(&gray, t, imageproc::contrast::ThresholdType::Binary);
    (SIG_OK, wrap_image(DynamicImage::ImageLuma8(result)))
}

pub static THRESHOLD: PrimitiveDef = PrimitiveDef {
    name: "image/threshold",
    func: prim_threshold,
    signal: Signal::errors(),
    arity: Arity::Exact(2),
    doc: "Binary threshold: pixels above t become 255, below become 0. Returns grayscale.",
    params: &["img", "threshold"],
    category: "image",
    example: "(image/threshold img 128)",
    aliases: &[],
};

// ── image/erode ─────────────────────────────────────────────────────

fn prim_erode(args: &[Value]) -> (SignalBits, Value) {
    let img = match get_image(&args[0], "image/erode") {
        Ok(i) => i,
        Err(e) => return e,
    };
    let radius = match require_int(&args[1], "image/erode", "radius") {
        Ok(v) => v.max(0) as u8,
        Err(e) => return e,
    };
    let gray = img.to_luma8();
    let result = morphology::erode(&gray, imageproc::distance_transform::Norm::LInf, radius);
    (SIG_OK, wrap_image(DynamicImage::ImageLuma8(result)))
}

pub static ERODE: PrimitiveDef = PrimitiveDef {
    name: "image/erode",
    func: prim_erode,
    signal: Signal::errors(),
    arity: Arity::Exact(2),
    doc: "Morphological erosion with given radius. Operates on grayscale.",
    params: &["img", "radius"],
    category: "image",
    example: "(image/erode img 2)",
    aliases: &[],
};

// ── image/dilate ────────────────────────────────────────────────────

fn prim_dilate(args: &[Value]) -> (SignalBits, Value) {
    let img = match get_image(&args[0], "image/dilate") {
        Ok(i) => i,
        Err(e) => return e,
    };
    let radius = match require_int(&args[1], "image/dilate", "radius") {
        Ok(v) => v.max(0) as u8,
        Err(e) => return e,
    };
    let gray = img.to_luma8();
    let result = morphology::dilate(&gray, imageproc::distance_transform::Norm::LInf, radius);
    (SIG_OK, wrap_image(DynamicImage::ImageLuma8(result)))
}

pub static DILATE: PrimitiveDef = PrimitiveDef {
    name: "image/dilate",
    func: prim_dilate,
    signal: Signal::errors(),
    arity: Arity::Exact(2),
    doc: "Morphological dilation with given radius. Operates on grayscale.",
    params: &["img", "radius"],
    category: "image",
    example: "(image/dilate img 2)",
    aliases: &[],
};
