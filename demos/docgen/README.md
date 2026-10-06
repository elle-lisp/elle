# Documentation site generator

<!-- audited: 2026-10-06 -->

An Elle program that renders the repository's markdown documents and an API reference as a static HTML site.

## Running

Run the generator from the repository root:

```bash
make docgen
```

The target builds the Rust API docs, then runs `elle demos/docgen/generate.lisp`.
The CI documentation job runs the same command. The generator writes the site
to `site/` in the working directory.

[generate.lisp](generate.lisp) names its inputs relative to the working
directory, so it must start at the repository root. Its own modules load through
`(import "./lib/...")`, which resolves against the directory of
`generate.lisp`, so they load wherever the program starts.

## Inputs

[docs/site.json](docs/site.json) holds the site's title, its description, its
home page and its sections. A section lists page names. A section with `dir`
reads its pages from that subdirectory of the repository's `docs/`. A section
with `"api": true` lists API pages instead of documents.

A document page is a markdown file under [docs/](../../docs/README.md). Its
first `# ` heading is the page title, and the paragraph under that heading is
the page description. The generator skips a page whose file is missing, and
prints a warning.

The API pages come from the running VM and from source:

| Page | Source |
|------|--------|
| `primitives` | `vm/list-primitives` and `vm/primitive-meta`, grouped by category |
| `prelude` | Each `(defmacro` in [src/prelude.lisp](../../src/prelude.lisp), described by the `## ` lines above it |
| `stdlib` | Each `(defn` in [src/stdlib.lisp](../../src/stdlib.lisp), under its `## ── Name ──` section, with the signals `compile/analyze` infers |
| `libraries` | Each `lib/*.lisp`, described by its first `## ` line, with its exports' signals |
| `plugins` | The README of each plugin under `plugins/`, rendered as markdown |

## Output

The generator writes `style.css` and one HTML file for each page. The home page
is `index.html`. A document page takes its name as its slug, or `DIR-NAME` in a
section with `dir`, or `DIR` for that section's `index` page. An API page is
`api-NAME.html`.

A link in a document to another markdown document becomes a link to that
page's `.html` file.

## Files

| File | Role |
|------|------|
| [generate.lisp](generate.lisp) | Configuration, navigation, the page template, link rewriting, and the main loop |
| [lib/markdown.lisp](lib/markdown.lisp) | Markdown to HTML: `parse`, `format-inline`, `html-escape` |
| [lib/css.lisp](lib/css.lisp) | The stylesheet, as one string |
| [lib/api.lisp](lib/api.lisp) | The API pages. It takes the site's helpers and paths, and answers one renderer for each page |
| [docs/site.json](docs/site.json) | The site's structure |

The markdown parser reads headings, code fences, tables, unordered lists,
blockquotes, horizontal rules and paragraphs. It has no ordered or nested
lists. Inline, it reads `**bold**`, `*italic*`, `` `code` `` and links.

## Adding a page

To add a document page, write the markdown file under `docs/`, then add its
name to a section in [docs/site.json](docs/site.json). To add an API page, write
its renderer in [lib/api.lisp](lib/api.lisp), add it to the struct that module
answers, and name it in the `api` section and in the two `cond` forms of
[generate.lisp](generate.lisp).
