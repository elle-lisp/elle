//! Drawing primitives for mutable @image.

use imageproc::drawing;

use elle::primitives::def::PrimitiveDef;
use elle::signals::Signal;
use elle::value::fiber::{SignalBits, SIG_OK};
use elle::value::types::Arity;
use elle::value::Value;

use crate::{extract_color, get_image_mut, require_int};

// ── image/draw-line ─────────────────────────────────────────────────

fn prim_draw_line(args: &[Value]) -> (SignalBits, Value) {
    let m = match get_image_mut(&args[0], "image/draw-line") {
        Ok(m) => m,
        Err(e) => return e,
    };
    let x1 = match require_int(&args[1], "image/draw-line", "x1") {
        Ok(v) => v as i32,
        Err(e) => return e,
    };
    let y1 = match require_int(&args[2], "image/draw-line", "y1") {
        Ok(v) => v as i32,
        Err(e) => return e,
    };
    let x2 = match require_int(&args[3], "image/draw-line", "x2") {
        Ok(v) => v as i32,
        Err(e) => return e,
    };
    let y2 = match require_int(&args[4], "image/draw-line", "y2") {
        Ok(v) => v as i32,
        Err(e) => return e,
    };
    let color = match extract_color(&args[5], "image/draw-line") {
        Ok(c) => c,
        Err(e) => return e,
    };
    let mut img = m.0.borrow_mut();
    let rgba = img.as_mut_rgba8().expect("draw requires rgba8 @image");
    drawing::draw_line_segment_mut(rgba, (x1 as f32, y1 as f32), (x2 as f32, y2 as f32), color);
    (SIG_OK, Value::NIL)
}

pub static DRAW_LINE: PrimitiveDef = PrimitiveDef {
    name: "image/draw-line",
    func: prim_draw_line,
    signal: Signal::errors(),
    arity: Arity::Exact(6),
    doc: "Draw a line on @image from (x1,y1) to (x2,y2) with color [r g b a].",
    params: &["img", "x1", "y1", "x2", "y2", "color"],
    category: "image",
    example: "(image/draw-line @img 0 0 100 100 [255 0 0 255])",
    aliases: &[],
};

// ── image/draw-rect ─────────────────────────────────────────────────

fn prim_draw_rect(args: &[Value]) -> (SignalBits, Value) {
    let m = match get_image_mut(&args[0], "image/draw-rect") {
        Ok(m) => m,
        Err(e) => return e,
    };
    let x = match require_int(&args[1], "image/draw-rect", "x") {
        Ok(v) => v as i32,
        Err(e) => return e,
    };
    let y = match require_int(&args[2], "image/draw-rect", "y") {
        Ok(v) => v as i32,
        Err(e) => return e,
    };
    let w = match require_int(&args[3], "image/draw-rect", "width") {
        Ok(v) => v as u32,
        Err(e) => return e,
    };
    let h = match require_int(&args[4], "image/draw-rect", "height") {
        Ok(v) => v as u32,
        Err(e) => return e,
    };
    let color = match extract_color(&args[5], "image/draw-rect") {
        Ok(c) => c,
        Err(e) => return e,
    };
    let mut img = m.0.borrow_mut();
    let rgba = img.as_mut_rgba8().expect("draw requires rgba8 @image");
    let rect = imageproc::rect::Rect::at(x, y).of_size(w, h);
    drawing::draw_hollow_rect_mut(rgba, rect, color);
    (SIG_OK, Value::NIL)
}

pub static DRAW_RECT: PrimitiveDef = PrimitiveDef {
    name: "image/draw-rect",
    func: prim_draw_rect,
    signal: Signal::errors(),
    arity: Arity::Exact(6),
    doc: "Draw a rectangle outline on @image.",
    params: &["img", "x", "y", "width", "height", "color"],
    category: "image",
    example: "(image/draw-rect @img 10 10 80 60 [0 255 0 255])",
    aliases: &[],
};

// ── image/draw-circle ───────────────────────────────────────────────

fn prim_draw_circle(args: &[Value]) -> (SignalBits, Value) {
    let m = match get_image_mut(&args[0], "image/draw-circle") {
        Ok(m) => m,
        Err(e) => return e,
    };
    let cx = match require_int(&args[1], "image/draw-circle", "cx") {
        Ok(v) => v as i32,
        Err(e) => return e,
    };
    let cy = match require_int(&args[2], "image/draw-circle", "cy") {
        Ok(v) => v as i32,
        Err(e) => return e,
    };
    let r = match require_int(&args[3], "image/draw-circle", "radius") {
        Ok(v) => v as i32,
        Err(e) => return e,
    };
    let color = match extract_color(&args[4], "image/draw-circle") {
        Ok(c) => c,
        Err(e) => return e,
    };
    let mut img = m.0.borrow_mut();
    let rgba = img.as_mut_rgba8().expect("draw requires rgba8 @image");
    drawing::draw_hollow_circle_mut(rgba, (cx, cy), r, color);
    (SIG_OK, Value::NIL)
}

pub static DRAW_CIRCLE: PrimitiveDef = PrimitiveDef {
    name: "image/draw-circle",
    func: prim_draw_circle,
    signal: Signal::errors(),
    arity: Arity::Exact(5),
    doc: "Draw a circle outline on @image.",
    params: &["img", "cx", "cy", "radius", "color"],
    category: "image",
    example: "(image/draw-circle @img 50 50 30 [0 0 255 255])",
    aliases: &[],
};

// ── image/fill-rect ─────────────────────────────────────────────────

fn prim_fill_rect(args: &[Value]) -> (SignalBits, Value) {
    let m = match get_image_mut(&args[0], "image/fill-rect") {
        Ok(m) => m,
        Err(e) => return e,
    };
    let x = match require_int(&args[1], "image/fill-rect", "x") {
        Ok(v) => v as i32,
        Err(e) => return e,
    };
    let y = match require_int(&args[2], "image/fill-rect", "y") {
        Ok(v) => v as i32,
        Err(e) => return e,
    };
    let w = match require_int(&args[3], "image/fill-rect", "width") {
        Ok(v) => v as u32,
        Err(e) => return e,
    };
    let h = match require_int(&args[4], "image/fill-rect", "height") {
        Ok(v) => v as u32,
        Err(e) => return e,
    };
    let color = match extract_color(&args[5], "image/fill-rect") {
        Ok(c) => c,
        Err(e) => return e,
    };
    let mut img = m.0.borrow_mut();
    let rgba = img.as_mut_rgba8().expect("draw requires rgba8 @image");
    let rect = imageproc::rect::Rect::at(x, y).of_size(w, h);
    drawing::draw_filled_rect_mut(rgba, rect, color);
    (SIG_OK, Value::NIL)
}

pub static FILL_RECT: PrimitiveDef = PrimitiveDef {
    name: "image/fill-rect",
    func: prim_fill_rect,
    signal: Signal::errors(),
    arity: Arity::Exact(6),
    doc: "Draw a filled rectangle on @image.",
    params: &["img", "x", "y", "width", "height", "color"],
    category: "image",
    example: "(image/fill-rect @img 10 10 80 60 [255 255 0 255])",
    aliases: &[],
};

// ── image/fill-circle ───────────────────────────────────────────────

fn prim_fill_circle(args: &[Value]) -> (SignalBits, Value) {
    let m = match get_image_mut(&args[0], "image/fill-circle") {
        Ok(m) => m,
        Err(e) => return e,
    };
    let cx = match require_int(&args[1], "image/fill-circle", "cx") {
        Ok(v) => v as i32,
        Err(e) => return e,
    };
    let cy = match require_int(&args[2], "image/fill-circle", "cy") {
        Ok(v) => v as i32,
        Err(e) => return e,
    };
    let r = match require_int(&args[3], "image/fill-circle", "radius") {
        Ok(v) => v as i32,
        Err(e) => return e,
    };
    let color = match extract_color(&args[4], "image/fill-circle") {
        Ok(c) => c,
        Err(e) => return e,
    };
    let mut img = m.0.borrow_mut();
    let rgba = img.as_mut_rgba8().expect("draw requires rgba8 @image");
    drawing::draw_filled_circle_mut(rgba, (cx, cy), r, color);
    (SIG_OK, Value::NIL)
}

pub static FILL_CIRCLE: PrimitiveDef = PrimitiveDef {
    name: "image/fill-circle",
    func: prim_fill_circle,
    signal: Signal::errors(),
    arity: Arity::Exact(5),
    doc: "Draw a filled circle on @image.",
    params: &["img", "cx", "cy", "radius", "color"],
    category: "image",
    example: "(image/fill-circle @img 50 50 30 [255 0 255 255])",
    aliases: &[],
};
