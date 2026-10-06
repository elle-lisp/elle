# A rest list in one region

<!-- audited: 2026-10-06 -->

When no cell of a variadic call's rest list can outlive the head, the list is built in one region instead of one region per cell.

A variadic callee receives its rest arguments as a fresh list, built by the
calling convention before the body runs. By default every cell of that list is
born in a region of its own, so a call with n rest arguments claims n pages
([model.md](model.md)). This document says when the list may share one region,
what proves it, and what the runtime does with the verdict.

## Why the default is one region per cell

A region frees all of its objects together, when its count reaches zero
([rules.md](rules.md), Rule 4). One region per cell lets each cell die at its
own last use. That matters when code keeps a tail of the list and drops the
head:

```lisp
(defn tail-of [& xs]
  "Every argument but the first."
  (rest xs))

(def t (tail-of 1 2 3))
(assert (= t (list 2 3)) "the tail outlives the call")
```

With one region per cell, the head cell of `xs` is freed when `tail-of`
returns, and the two cells of `t` live on. With one region for the whole list,
`t` holds that region, and the dead head cell stays until `t` dies. That is a
value kept past its last reference, which Rule 8 forbids ([rules.md](rules.md)).
So a rest list may share one region only when every cell of it dies at the same
point.

## When every cell dies with the head

The calling convention hands the callee one reference to the head, and the
callee releases it at the rest parameter's last use. A tail of the list is
reachable from the head and from nothing else, until some code takes the tail
out. If the callee never takes a tail out, every cell dies when the head does,
and one region frees them at exactly the point one region per cell would.

The compiler proves that per lambda, over the canonical HIR the region
analysis reads ([escape.md](../escape.md)). The rest list is admitted to one
region when its parameter is never reassigned, no nested lambda captures it,
and every reference to it is one of these:

- an argument to a primitive that declares `Immediate`
  ([effects.md](effects.md)), such as `length`, `empty?` or `=`: the result
  is never a cell, and the primitive keeps no argument;
- the argument of `first` or `second`, whose built-in method answers an
  element of the list, never a cell of it;
- an operand of `%first`, `%length`, `%eq`, `%ne`, `%identical?`, `%not`,
  `%type-of` or a type predicate such as `%pair?`;
- a spliced argument, `(f ;xs)`, which is what `apply` expands to: the callee
  receives the elements, not the cells.

Anything else refuses the list, and it keeps one region per cell. The refusals
include returning the list, storing it, binding it to another name, passing it
unspliced to a closure or to any other primitive, destructuring or matching
it, and calling `rest` or `%rest` on it. A refusal costs pages and nothing
else, because one region per cell is always correct.

```lisp
(defn count-args [& xs]
  "Admitted: the list is read by length alone."
  (length xs))

(defn ordered? [a b & more]
  "Admitted: empty? and a spliced call read the list."
  (if (empty? more) (< a b) (and (< a b) (ordered? b ;more))))

(assert (= (count-args 1 2 3) 3) "the count")
(assert (ordered? 1 2 3 4) "ascending")
(assert (not (ordered? 1 3 2)) "not ascending")
```

`+` is refused: it hands its rest list to the `letrec` helper that walks it, a
call to a closure.

### The default traitsets are trusted

`first`, `second`, `length` and `empty?` run the method the list's trait table
names. A new cell takes the heap's default traitset for pairs, and a program
may replace a method in that traitset for every list on the heap
([traits](../../traits.md)). The gate reads the built-in methods' behavior and
does not guard against a replacement. A replacement method that keeps a cell of
a rest list keeps that list's region until its last cell dies. The free is
late and never early, so a replaced method costs promptness and never
soundness.

## What the runtime does

The verdict rides the code object as the rest list's layout: one region per
cell, or one region for the list ([template.md](template.md)). The interpreter's
environment builder and the JIT's entry block both read it.

For a list in one region, the runtime mints one region and builds the cells
into it, last argument first. Each cell's `rest` points at the cell built
before it, in the same region, so the allocation scan counts no reference for
it and the free cascade decrements none. The head carries the region's one
reference, and its release frees every cell. An element that lives in another
region keeps the counted edge it always had.

An empty rest list is the empty list and mints nothing. A one-argument rest
list is one cell, which takes one region under either layout. The saving is
the n − 1 page claims of a list of n cells.

The WASM full-module host builds its own rest lists, already in one region,
and runs no region release during a program ([diagnostics.md](diagnostics.md)).
It does not read the layout.

## Tests

[region-rest-list.lisp](../../../tests/impl/region-rest-list.lisp) measures
both halves. An admitted list claims the same pages whatever its length. A
refused list frees its head when a kept tail outlives it, one shape per refusal.
The Rust tests beside the gate pin each admission and each refusal.
