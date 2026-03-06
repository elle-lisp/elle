# Documentation Site Generator

The Elle documentation site generator is a pure-Elle program that generates static HTML documentation from JSON input files and Elle runtime metadata.

## What It Does

This generator:
1. **Reads hand-curated JSON pages** from `docs/pages/` (index, getting-started, language-guide, examples, concurrency)
2. **Auto-generates stdlib reference** from `(vm/list-primitives)` and `(vm/primitive-meta)` — the complete list of built-in functions with their documentation
3. **Applies HTML templating** using the `lib/` modules
4. **Generates static HTML pages** organized by category
5. **Produces a complete documentation site** in the `site/` directory

## How to Run

### Using Make

```bash
make docgen
```

### Manual Build

```bash
cargo build --release
./target/release/elle demos/docgen/generate.lisp
```

This generates the documentation site in `site/` with the following structure:

```
site/
├── index.html              # Home page
├── getting-started.html    # Getting started guide
├── language-guide.html     # Language reference
├── examples.html           # Example programs
├── concurrency.html        # Concurrency and fibers
├── stdlib-reference.html   # Complete stdlib documentation
└── assets/
    └── style.css           # Stylesheet
```

## How It Works

### Input Files

**Hand-curated JSON pages** (`docs/pages/*.json`):
- `index.json` — Home page with feature overview
- `getting-started.json` — Installation and first program
- `language-guide.json` — Language syntax and semantics
- `examples.json` — Example programs demonstrating features
- `concurrency.json` — Fibers, signals, and concurrency
- `stdlib-reference.json` — Placeholder; auto-generated from runtime metadata

**Site configuration** (`docs/site.json`):
- Navigation structure
- Page metadata
- Site-wide settings

### Generation Process

1. **Read JSON input** — Parse `docs/site.json` and `docs/pages/*.json`
2. **Extract stdlib metadata** — Call `(vm/list-primitives)` to get all built-in functions
3. **Generate HTML** — Apply templates from `lib/` to create HTML pages
4. **Write output** — Save HTML files to `site/`

### Library Modules

The `lib/` directory contains reusable Elle code for generation:

| File | Purpose |
|------|---------|
| `html.lisp` | HTML generation utilities (tags, escaping, attributes) |
| `css.lisp` | CSS stylesheet generation (inline styles) |
| `content.lisp` | Content transformation (markdown → HTML, formatting) |
| `template.lisp` | Page templates (header, footer, navigation) |
| `AGENTS.md` | Documentation for library developers |

Each module exports functions used by `generate.lisp` to build the final site.

## Adding New Pages

To add a new documentation page:

1. **Create a JSON file** in `docs/pages/your-page.json`:

```json
{
  "title": "Your Page Title",
  "description": "Short description for metadata",
  "sections": [
    {
      "heading": "Section Title",
      "level": 2,
      "content": [
        {
          "type": "paragraph",
          "text": "Your content here. Supports **bold**, *italic*, and `code`."
        },
        {
          "type": "code",
          "language": "lisp",
          "content": "(+ 1 2)"
        }
      ]
    }
  ]
}
```

2. **Register the page** in `docs/site.json`:

```json
{
  "pages": [
    {
      "id": "your-page",
      "title": "Your Page Title",
      "path": "your-page.html"
    }
  ]
}
```

3. **Run the generator**:

```bash
make docgen
```

## Updating Existing Content

### Hand-curated pages

Edit the JSON file directly in `docs/pages/`:
- Change text content
- Add/remove sections
- Update code examples
- Modify metadata

Then regenerate:

```bash
make docgen
```

### Stdlib reference

The stdlib reference is **auto-generated** from the Elle runtime. To update it:

1. **Add/modify primitives** in `src/primitives/`
2. **Add docstrings** to primitive functions
3. **Run the generator** — it will automatically include the new primitives

The generator calls:
- `(vm/list-primitives)` — Get all primitive names
- `(vm/primitive-meta name)` — Get metadata (arity, effect, docstring) for each primitive

## Important Notes

### List Termination

The generator uses Elle's list operations extensively. **Critical invariant:**

- Lists terminate with `EMPTY_LIST`, not `NIL`
- Use `empty?` to check for end-of-list, not `nil?`
- `(rest (list 1))` returns `EMPTY_LIST` (truthy), not `NIL` (falsy)

Using `nil?` instead of `empty?` causes infinite loops. See `AGENTS.md` for details.

### HTML Escaping

All user-provided content is escaped using `html-escape` to prevent injection attacks. The generator is safe to use with untrusted input.

### Deterministic Output

The generator produces the same output for the same input every time. This makes it suitable for CI/CD pipelines and documentation versioning.

## Troubleshooting

### Generator hangs or times out

Check for:
- Infinite loops in recursive functions (use `empty?`, not `nil?`)
- Missing input files in `docs/pages/`
- Circular dependencies in library modules

### Missing or incomplete output

Check for:
- Errors in JSON syntax (use a JSON validator)
- Missing `docs/site.json` configuration
- Errors in `lib/` modules (check Elle error messages)

### Malformed HTML

Check for:
- Unclosed tags in templates
- Incorrect string concatenation
- Missing HTML escaping (should use `html-escape`)

## Development

To modify the generator:

1. **Edit `generate.lisp`** for main generation logic
2. **Edit `lib/*.lisp`** for utility functions
3. **Edit `docs/pages/*.json`** for content
4. **Edit `docs/site.json`** for site structure
5. **Run `make docgen`** to test changes
6. **Check `site/`** for generated output

## CI Integration

The generator runs as part of the documentation CI job:

```bash
cargo build --release
./target/release/elle demos/docgen/generate.lisp
```

If the docs job fails, check:
1. The error message from the Elle runtime
2. The JSON input files for syntax errors
3. The `lib/` modules for logic errors
4. The list termination logic (use `empty?`, not `nil?`)

See `AGENTS.md` for more details on failure triage.
