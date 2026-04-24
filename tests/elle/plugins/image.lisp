#!/usr/bin/env elle
(elle/epoch 9)

## Test suite for image plugin

(def [ok? img] (protect (import "plugin/image")))
(when (not ok?)
  (println "SKIP: image plugin not built")
  (exit 0))

# ── Create image from pixels ─────────────────────────────────────────

## 2x2 red RGBA8 image
(def red-pixel [255 0 0 255])
(def red-bytes b[255 0 0 255  255 0 0 255  255 0 0 255  255 0 0 255])
(def red-img (img:from-pixels 2 2 :rgba8 red-bytes))

(assert (= (img:width red-img) 2) "width")
(assert (= (img:height red-img) 2) "height")
(assert (= (img:dimensions red-img) [2 2]) "dimensions")
(assert (= (img:color-type red-img) :rgba8) "color-type")

# ── Pixel access ─────────────────────────────────────────────────────

(def px (img:get-pixel red-img 0 0))
(assert (= px [255 0 0 255]) "get-pixel red")

(def blue-img (img:put-pixel red-img 1 1 [0 0 255 255]))
(assert (= (img:get-pixel blue-img 1 1) [0 0 255 255]) "put-pixel immutable")

# ── Mutable image ────────────────────────────────────────────────────

(def mimg (img:new 4 4 :rgba8))
(assert (= (img:width mimg) 4) "@image width")
(img:put-pixel mimg 0 0 [128 64 32 255])
(assert (= (img:get-pixel mimg 0 0) [128 64 32 255]) "put-pixel mutable")

# ── Thaw / Freeze ────────────────────────────────────────────────────

(def thawed (img:thaw red-img))
(img:put-pixel thawed 0 0 [0 255 0 255])
(assert (= (img:get-pixel thawed 0 0) [0 255 0 255]) "thaw then mutate")

## Original unchanged
(assert (= (img:get-pixel red-img 0 0) [255 0 0 255]) "original unchanged after thaw")

(def frozen (img:freeze thawed))
(assert (= (img:get-pixel frozen 0 0) [0 255 0 255]) "freeze snapshot")

# ── Transforms ───────────────────────────────────────────────────────

(def resized (img:resize red-img 4 4 :nearest))
(assert (= (img:dimensions resized) [4 4]) "resize")

(def cropped (img:crop resized 1 1 2 2))
(assert (= (img:dimensions cropped) [2 2]) "crop")

(def rotated (img:rotate red-img :r90))
(assert (= (img:dimensions rotated) [2 2]) "rotate r90")

(def flipped (img:flip red-img :h))
(assert (= (img:dimensions flipped) [2 2]) "flip h")

# ── Adjustments ──────────────────────────────────────────────────────

(def gray (img:grayscale red-img))
(assert (or (= (img:color-type gray) :luma8) (= (img:color-type gray) :lumaa8)) "grayscale → luma")

(def inv (img:invert red-img))
(assert (not (nil? inv)) "invert runs")

(def blurred (img:blur red-img 1.0))
(assert (not (nil? blurred)) "blur runs")

# ── Color conversion ─────────────────────────────────────────────────

(def as-rgb (img:to-rgb8 red-img))
(assert (= (img:color-type as-rgb) :rgb8) "to-rgb8")

(def as-rgba (img:to-rgba8 as-rgb))
(assert (= (img:color-type as-rgba) :rgba8) "to-rgba8")

# ── Encode / Decode roundtrip ────────────────────────────────────────

(def png-bytes (img:encode red-img :png))
(assert (> (length png-bytes) 0) "encode produces bytes")

(def decoded (img:decode png-bytes :png))
(assert (= (img:dimensions decoded) [2 2]) "decode roundtrip dimensions")
(assert (= (img:get-pixel decoded 0 0) [255 0 0 255]) "decode roundtrip pixel")

# ── Drawing ──────────────────────────────────────────────────────────

(def canvas (img:new 100 100 :rgba8))
(img:fill-rect canvas 10 10 20 20 [255 0 0 255])
(assert (= (img:get-pixel canvas 15 15) [255 0 0 255]) "fill-rect")
(assert (= (img:get-pixel canvas 0 0) [0 0 0 0]) "outside fill-rect")

(img:draw-line canvas 0 0 99 99 [0 255 0 255])
(img:draw-rect canvas 5 5 90 90 [0 0 255 255])
(img:draw-circle canvas 50 50 20 [255 255 0 255])
(img:fill-circle canvas 50 50 5 [255 0 255 255])

# ── Compositing ──────────────────────────────────────────────────────

(def base (img:new 10 10 :rgba8))
(img:fill-rect base 0 0 10 10 [100 100 100 255])
(def base-frozen (img:freeze base))

(def overlay (img:new 5 5 :rgba8))
(img:fill-rect overlay 0 0 5 5 [200 200 200 255])
(def overlay-frozen (img:freeze overlay))

(def composited (img:overlay base-frozen overlay-frozen 2 2))
(assert (= (img:dimensions composited) [10 10]) "overlay dimensions")

(def blended (img:blend base-frozen base-frozen 0.5))
(assert (= (img:dimensions blended) [10 10]) "blend dimensions")

# ── Analysis ─────────────────────────────────────────────────────────

(def hist (img:histogram red-img))
(assert (= (length (get hist :r)) 256) "histogram 256 bins")

(def edges (img:edges red-img))
(assert (not (nil? edges)) "edges runs")

(def thresh (img:threshold red-img 128))
(assert (= (img:color-type thresh) :luma8) "threshold → luma8")

(println "image: all tests passed")
