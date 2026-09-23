// audited: 2026-09-23
//! Built-in documentation for special forms and prelude macros, and the text `(help)` prints.
//!
//! docs/stdlib.md
//!
//! Special forms take their docs from the analyzer's registry. The prelude
//! macros and the expander's `defmacro` have no registry entry, so their docs
//! are the hand-written `HAND_WRITTEN` table, whose examples a test runs.

use super::def::Doc;
use crate::signals::Signal;
use crate::value::types::Arity;

/// Docs for the forms the special-form registry does not carry: the
/// expander's `defmacro` and the prelude macros. Every example runs as a
/// program (`tests::every_hand_written_example_runs`).
pub(crate) const HAND_WRITTEN: &[Doc] = &[
    Doc {
        name: "defmacro",
        doc: "Define a macro. The expander calls it with the unevaluated argument forms as syntax objects, and compiles the form it returns in their place.",
        params: &["name", "(params...)", "body..."],
        arity: Arity::AtLeast(2),
        signal: Signal::silent(),
        category: "special form",
        example: "(defmacro unless-zero (n & body)\n  `(if (= ,n 0) nil (begin ,;body)))\n(unless-zero 3 :nonzero)  # => :nonzero",
        aliases: &[],
    },
    Doc {
        name: "each",
        doc: "Iterate over a sequence, binding each element to a name. Walks a list, an array, a string, bytes, a fiber's yields, a set, a struct's pairs, or a value whose traits carry :iter. The `in` is optional.",
        params: &["name", "in?", "collection", "body..."],
        arity: Arity::AtLeast(2),
        signal: Signal::silent(),
        category: "syntax sugar",
        example: "(each x in [1 2 3]\n  (println x))",
        aliases: &[],
    },
    Doc {
        name: "yield",
        doc: "Suspend the current fiber, handing value (nil when omitted) to its resumer. Expands to (emit :yield value).",
        params: &["value?"],
        arity: Arity::Range(0, 1),
        signal: Signal::yields(),
        category: "syntax sugar",
        example: "(def gen (fiber/new (fn () (yield 1) (yield 2)) |:yield|))\n(fiber/resume gen)  # => 1",
        aliases: &[],
    },
    Doc {
        name: "defn",
        doc: "Define a named function. Shorthand for (def name (fn params body...)).",
        params: &["name", "params", "body..."],
        arity: Arity::AtLeast(2),
        signal: Signal::silent(),
        category: "syntax sugar",
        example: "(defn add [x y] (+ x y))\n(add 1 2)  # => 3",
        aliases: &[],
    },
    Doc {
        name: "let*",
        doc: "An alias of let, kept for Scheme readers: bindings are flat name-value pairs, and each sees the ones before it.",
        params: &["[name value ...]", "body..."],
        arity: Arity::AtLeast(1),
        signal: Signal::silent(),
        category: "syntax sugar",
        example: "(let* [x 1 y (+ x 1)] (+ x y))  # => 3",
        aliases: &[],
    },
    Doc {
        name: "->",
        doc: "Thread-first macro. Inserts value as first argument of each successive form.",
        params: &["value", "forms..."],
        arity: Arity::AtLeast(1),
        signal: Signal::silent(),
        category: "syntax sugar",
        example: "(-> 5 (+ 3) (* 2))  # => (* (+ 5 3) 2) => 16",
        aliases: &[],
    },
    Doc {
        name: "->>",
        doc: "Thread-last macro. Inserts value as last argument of each successive form.",
        params: &["value", "forms..."],
        arity: Arity::AtLeast(1),
        signal: Signal::silent(),
        category: "syntax sugar",
        example: "(->> 5 (- 10) (* 2))  # => (* 2 (- 10 5)) => 10",
        aliases: &[],
    },
    Doc {
        name: "when",
        doc: "Evaluate body when condition is true. Returns nil if false.",
        params: &["condition", "body..."],
        arity: Arity::AtLeast(1),
        signal: Signal::silent(),
        category: "syntax sugar",
        example: "(when (> 3 0) (println \"positive\"))",
        aliases: &[],
    },
    Doc {
        name: "unless",
        doc: "Evaluate body when condition is false. Returns nil if true.",
        params: &["condition", "body..."],
        arity: Arity::AtLeast(1),
        signal: Signal::silent(),
        category: "syntax sugar",
        example: "(unless (empty? [1 2]) (first [1 2]))  # => 1",
        aliases: &[],
    },
    Doc {
        name: "error",
        doc: "Signal an error. The value can be anything; by convention a struct {:error :kind :message \"msg\"}. With no argument, signals nil.",
        params: &["value?"],
        arity: Arity::Range(0, 1),
        signal: Signal::errors(),
        category: "syntax sugar",
        example: "(protect (error {:error :not-found :message \"missing key\"}))\n# => [false {:error :not-found :message \"missing key\"}]",
        aliases: &[],
    },
    Doc {
        name: "try",
        doc: "Run body in a fiber. If it signals an error, run the handler with the error bound to the catch name and return the handler's value.",
        params: &["body...", "(catch name handler...)"],
        arity: Arity::AtLeast(1),
        signal: Signal::silent(),
        category: "syntax sugar",
        example: "(try (error {:error :boom}) (catch e (get e :error)))  # => :boom",
        aliases: &[],
    },
    Doc {
        name: "protect",
        doc: "Run body in a fiber and return [ok? value]: [true result] when it completes, [false error] when it signals an error. The error does not propagate.",
        params: &["body..."],
        arity: Arity::AtLeast(1),
        signal: Signal::silent(),
        category: "syntax sugar",
        example: "(protect (+ 1 2))                 # => [true 3]\n(protect (error {:error :boom}))  # => [false {:error :boom}]",
        aliases: &[],
    },
    Doc {
        name: "defer",
        doc: "Run body in a fiber, then run cleanup whether the body completed or failed. Returns the body's value, or propagates its error after the cleanup.",
        params: &["cleanup", "body..."],
        arity: Arity::AtLeast(2),
        signal: Signal::silent(),
        category: "syntax sugar",
        example: "(def log @[])\n(defer (push log :cleanup)\n  (push log :body)\n  42)  # => 42, and log is @[:body :cleanup]",
        aliases: &[],
    },
    Doc {
        name: "with",
        doc: "Bind name to the value of ctor, run body, then call dtor on that value, even when the body fails. Built on defer.",
        params: &["name", "ctor", "dtor", "body..."],
        arity: Arity::AtLeast(4),
        signal: Signal::silent(),
        category: "syntax sugar",
        example: "(def closed @[])\n(with xs @[1 2] (fn [v] (push closed v))\n  (length xs))  # => 2, and closed holds xs",
        aliases: &[],
    },
    Doc {
        name: "yield*",
        doc: "Resume a sub-fiber until it finishes, re-yielding each value it yields and passing each resume value back in. Returns the sub-fiber's final value.",
        params: &["fiber"],
        arity: Arity::Exact(1),
        signal: Signal::yields(),
        category: "syntax sugar",
        example: "(def inner (fiber/new (fn () (yield 1) 2) |:yield|))\n(def outer (fiber/new (fn () (yield* inner)) |:yield|))\n(fiber/resume outer)  # => 1",
        aliases: &[],
    },
    Doc {
        name: "ffi/defbind",
        doc: "Define a named wrapper for a C function via FFI. Looks up the symbol, creates a signature, and defines a function that calls it.",
        params: &["name", "lib-handle", "\"c-name\"", "return-type", "[arg-types...]"],
        arity: Arity::Exact(5),
        signal: Signal::silent(),
        category: "syntax sugar",
        example: "(def libc (ffi/native nil))\n(ffi/defbind abs libc \"abs\" :int [:int])\n(abs -3)  # => 3",
        aliases: &[],
    },
];

/// Register documentation for special forms and prelude macros.
///
/// These aren't primitives (no NativeFn) but they should be discoverable
/// via `(doc "if")`, `(doc "defn")`, etc. Called during `register_primitives`.
pub(crate) fn register_builtin_docs(docs: &mut std::collections::HashMap<String, Doc>) {
    // Special forms come from the analyzer's registry
    // (hir::analyze::forms::registry) — the single source of truth for
    // special-form names and metadata. Internal forms are not documented.
    for form in crate::hir::analyze::forms::registry::SPECIAL_FORMS {
        if form.internal {
            continue;
        }
        let doc = Doc {
            name: form.name,
            doc: form.doc,
            params: form.params,
            arity: form.arity,
            signal: form.signal,
            category: "special form",
            example: form.example,
            aliases: form.aliases,
        };
        for name in std::iter::once(form.name).chain(form.aliases.iter().copied()) {
            docs.insert(name.to_string(), doc.clone());
        }
    }

    for doc in HAND_WRITTEN {
        docs.insert(doc.name.to_string(), doc.clone());
    }
}

/// Append `names` to `out` as a comma-separated list, indented two spaces and
/// wrapped before 76 columns.
fn push_wrapped(out: &mut String, names: &[&str]) {
    let mut line = String::from(" ");
    for (i, name) in names.iter().enumerate() {
        let item = if i + 1 < names.len() {
            format!(" {name},")
        } else {
            format!(" {name}")
        };
        if line.len() + item.len() > 76 {
            out.push_str(&line);
            out.push('\n');
            line = String::from(" ");
        }
        line.push_str(&item);
    }
    out.push_str(&line);
    out.push('\n');
}

/// Generate help text from the primitive definition tables.
///
/// Groups primitives by category, showing name and doc for each. The special
/// forms come from the analyzer's registry and the syntax sugar from
/// `HAND_WRITTEN`, so neither list is kept by hand.
pub fn help_text() -> String {
    use std::collections::BTreeMap;

    let mut categories: BTreeMap<&str, Vec<(&str, &str)>> = BTreeMap::new();

    for table in super::registration::ALL_TABLES {
        for def in *table {
            let cat = if def.category.is_empty() {
                "core"
            } else {
                def.category
            };
            categories.entry(cat).or_default().push((def.name, def.doc));
        }
    }

    let mut out = String::new();
    out.push_str("Primitives:\n");

    for (category, prims) in &categories {
        // Capitalize category name
        let display_name: String = {
            let mut chars = category.chars();
            match chars.next() {
                None => String::new(),
                Some(c) => c.to_uppercase().collect::<String>() + chars.as_str(),
            }
        };

        // Collect just the names, join with ", "
        let names: Vec<&str> = prims.iter().map(|(name, _)| *name).collect();
        out.push_str(&format!("  {:14} {}\n", display_name, names.join(", ")));
    }

    let special: Vec<&str> = crate::hir::analyze::forms::registry::SPECIAL_FORMS
        .iter()
        .filter(|f| !f.internal)
        .map(|f| f.name)
        .chain(
            HAND_WRITTEN
                .iter()
                .filter(|d| d.category == "special form")
                .map(|d| d.name),
        )
        .collect();
    let sugar: Vec<&str> = HAND_WRITTEN
        .iter()
        .filter(|d| d.category == "syntax sugar")
        .map(|d| d.name)
        .collect();

    out.push_str("\nSpecial forms:\n");
    push_wrapped(&mut out, &special);
    out.push_str("\nSyntax sugar:\n");
    push_wrapped(&mut out, &sugar);
    out.push_str("\nREPL commands:\n");
    out.push_str("  (help)         Show this help\n");
    out.push_str("  (doc \"name\")   Show documentation for any named form\n");
    out.push_str("  (exit)         Exit the REPL\n");

    out
}
