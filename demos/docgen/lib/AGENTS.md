# elle-doc/lib

Elle library modules for the documentation site generator.

## Responsibility

Provide reusable Elle functions for the documentation generator (`generate.lisp`):
- HTML generation and escaping
- CSS stylesheet generation
- Content block rendering (paragraphs, code, lists, tables, etc.)
- Page template generation

## Modules

| Module | Purpose |
|--------|---------|
| `html.lisp` | HTML escaping and utility functions |
| `css.lisp` | CSS stylesheet generation |
| `content.lisp` | Content block rendering (paragraphs, code, lists, tables, blockquotes, notes) |
| `template.lisp` | Page template generation (navigation, full HTML pages) |

## Key functions

### html.lisp

**`html-escape(str)`** — Escape HTML special characters
- Converts `&` → `&amp;`, `<` → `&lt;`, `>` → `&gt;`, `"` → `&quot;`, `'` → `&#39;`
- Used to prevent HTML injection in user-provided content

### css.lisp

**`generate-css()`** — Generate inline CSS stylesheet
- Returns a complete CSS string with:
  - CSS variables for colors (light/dark mode support)
  - Layout styles (sidebar, content area)
  - Typography and spacing
  - Code block styling
  - Note/callout styling
  - Responsive design

### content.lisp

**`render-paragraph(block)`** — Render a paragraph block to HTML
- Input: `{:type "paragraph" :text "..."}`
- Output: `<p>...</p>`

**`render-code(block)`** — Render a code block to HTML
- Input: `{:type "code" :text "..." :language "lisp"}`
- Output: `<pre><code class="language-lisp">...</code></pre>`

**`render-list(block)`** — Render a list block to HTML
- Input: `{:type "list" :items [...] :ordered true/false}`
- Output: `<ol>...</ol>` or `<ul>...</ul>`

**`render-blockquote(block)`** — Render a blockquote block to HTML
- Input: `{:type "blockquote" :text "..."}`
- Output: `<blockquote>...</blockquote>`

**`render-table(block)`** — Render a table block to HTML
- Input: `{:type "table" :headers [...] :rows [...]}`
- Output: `<table><thead>...</thead><tbody>...</tbody></table>`

**`render-note(block)`** — Render a note/callout block to HTML
- Input: `{:type "note" :text "..." :kind "info|warning|tip"}`
- Output: `<div class="note note-info">...</div>`

**`render-block(block)`** — Main dispatcher for rendering any block type
- Dispatches to the appropriate `render-*` function based on block type
- Returns empty string for unknown types

### template.lisp

**`generate-nav(nav-items, current-slug)`** — Generate navigation HTML
- Input: array of `{:slug "..." :title "..."}` items, current page slug
- Output: `<li><a href="...">...</a></li>` for each item
- Marks current page with `active` class

**`generate-page(site, page, nav, css, body)`** — Generate complete HTML page
- Input:
  - `site`: `{:title "..." :nav [...]}`
  - `page`: `{:title "..." :description "..." :slug "..."}`
  - `nav`: CSS stylesheet string
  - `css`: CSS stylesheet string
  - `body`: HTML body content
- Output: Complete HTML5 document with:
  - DOCTYPE and meta tags
  - Inline CSS
  - Sidebar navigation
  - Main content area
  - Responsive layout

## How they compose

The generator (`generate.lisp`) uses these modules in sequence:

1. **Load modules** — `(import-file "elle-doc/lib/html.lisp")` etc.
2. **Generate CSS** — `(generate-css)` produces the stylesheet
3. **Render content** — For each content block, call `render-block()`
4. **Generate page** — `generate-page()` wraps content in HTML template
5. **Write output** — Save HTML to file

## Important invariants

1. **HTML must be escaped.** All user-provided content must pass through `html-escape()` to prevent injection attacks.

2. **Lists terminate with `EMPTY_LIST`.** Use `empty?` to check for end-of-list, not `nil?`. This is critical for the `fold` operations in `content.lisp` and `template.lisp`.

3. **String concatenation uses `->` and `append`.** The `->` macro threads the first argument through subsequent calls, and `append` concatenates strings.

4. **CSS is inline.** Styles are generated as a single CSS string and embedded in the `<style>` tag, not as external stylesheets.

5. **Navigation is generated from site metadata.** The `generate-nav()` function builds the sidebar from the site's nav items array.

## Common patterns

### Rendering with fold

Content blocks often use `fold` to accumulate rendered items:

```janet
(fold
  (fn (acc item)
    (-> acc (append "<li>") (append (html-escape item)) (append "</li>")))
  ""
  items)
```

This pattern:
1. Starts with empty string accumulator
2. For each item, appends HTML tags and escaped content
3. Returns the concatenated result

### Threading with ->

The `->` macro threads values through function calls:

```janet
(-> "<p>" (append (format-inline text)) (append "</p>"))
```

Expands to:

```janet
(append (append "<p>" (format-inline text)) "</p>")
```

## Dependents

- `generate.lisp` — Main generator script imports and uses all modules
- Elle documentation site generation — CI runs `./target/release/elle elle-doc/generate.lisp`

## Files

| File | Lines | Content |
|------|-------|---------|
| `html.lisp` | 7 | HTML escaping utility |
| `css.lisp` | 303 | CSS stylesheet generation |
| `content.lisp` | 127 | Content block rendering |
| `template.lisp` | 46 | Page template generation |
