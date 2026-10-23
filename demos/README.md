# Elle Demos

<!-- audited: 2026-10-23 -->

Programs that dogfood Elle on work larger than a test, several of them beside
the same program in another language.

A demo shows what Elle can express and where it hurts: a missing feature, a
slow path, a form that reads badly. A demo is not a test. The suites hold the
claims ([testing](../docs/testing.md)), and a demo prints what it computed.
Each directory's README says what the demo does, what it needs, and how to run
it. A demo that needs no plugin and no display runs from the repository root
as `elle demos/NAME/NAME.lisp`.

## Writing a demo

- Open the file with a comment that says what the demo shows.
- Import what it needs, then define helpers with `defn`, then run the work.
- Print the results with `println`, so a person running it sees them.
- Let an error propagate. A demo that catches its own failures hides them.
- Prefer the forms a reader of other Elle code meets: `->` and `->>` for
  threading, `when` and `unless`, `fold`, `map` and `filter`.
- Give the directory a README with a title and a one-sentence call-out, so the
  index lists it ([the generated index](../docs/impl/agents-index.md)).
