(import-file "tests/elle/assert.lisp")

## fn/cfg integration tests - Part 3
## Tests the new field tests and Mermaid visual features

## ── New field tests ─────────────────────────────────────────────────

(def flow1 (fn/flow (fn (o) o)))
(def block1 (get (get flow1 :blocks) 0))
(assert-true (tuple? (get block1 :display)) "fn/flow block has display")

(def flow2 (fn/flow (fn (p) p)))
(def block2 (get (get flow2 :blocks) 0))
(def display2 (get block2 :display))
(assert-true (string/contains? (get display2 0) "r") "fn/flow display is compact")

(def flow3 (fn/flow (fn (q) q)))
(def block3 (get (get flow3 :blocks) 0))
(assert-true (keyword? (get block3 :term-kind)) "fn/flow block has term-kind")

(def flow4 (fn/flow (fn (r) r)))
(def block4 (get (get flow4 :blocks) 0))
(assert-eq (get block4 :term-kind) :return "fn/flow term-kind return")

(def flow5 (fn/flow (fn (s) (if s 1 2))))
(def entry5 (get (get flow5 :blocks) 0))
(assert-eq (get entry5 :term-kind) :branch "fn/flow term-kind branch")

(def flow6 (fn/flow (fn (t) t)))
(def block6 (get (get flow6 :blocks) 0))
(assert-true (string? (get block6 :term-display)) "fn/flow block has term-display")

(def flow7 (fn/flow (fn (u) u)))
(def block7 (get (get flow7 :blocks) 0))
(assert-true (string/starts-with? (get block7 :term-display) "return") "fn/flow term-display compact")

## ── Mermaid visual feature tests ────────────────────────────────────

(def r16 (fn/cfg (fn (v) v) :mermaid))
(assert-true (string/contains? r16 "classDef") "fn/cfg mermaid has classdef")

(def r17 (fn/cfg (fn (w) (if w 1 2)) :mermaid))
(assert-true (string/contains? r17 "{") "fn/cfg mermaid branch uses diamond")

(def r18 (fn/cfg (fn (z) z) :mermaid))
(assert-true (string/contains? r18 "([") "fn/cfg mermaid return uses stadium")

(def r19 (fn/cfg (fn (aa) aa) :mermaid))
(assert-true (not (string/contains? r19 "Reg(")) "fn/cfg mermaid compact instructions")
