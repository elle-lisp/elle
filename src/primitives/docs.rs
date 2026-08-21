//! Built-in documentation for special forms and prelude macros.

use super::def::Doc;

/// Register documentation for special forms and prelude macros.
///
/// These aren't primitives (no NativeFn) but they should be discoverable
/// via `(doc "if")`, `(doc "defn")`, etc. Called during `register_primitives`.
pub(crate) fn register_builtin_docs(docs: &mut std::collections::HashMap<String, Doc>) {
    use crate::signals::Signal;
    use crate::value::types::Arity;

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

    let builtins: &[Doc] = &[
        // === Special forms not handled by the analyzer (expander/prelude) ===
        Doc {
            name: "each",
            doc: "Iterate over a list, binding each element to a name.",
            params: &["(name list)", "body..."],
            arity: Arity::AtLeast(1),
            signal: Signal::silent(),
            category: "special form",
            example: "(each (x (list 1 2 3)) (display x))",
            aliases: &[],
        },
        Doc {
            name: "yield",
            doc: "Yield a value from a fiber. Suspends execution until resumed.",
            params: &["value"],
            arity: Arity::Exact(1),
            signal: Signal::yields(),
            category: "special form",
            example: "(fn () (yield 1) (yield 2) (yield 3))",
            aliases: &[],
        },
        Doc {
            name: "defmacro",
            doc: "Define a syntax macro. The macro function receives syntax objects and returns a syntax object.",
            params: &["name", "(params...)", "body..."],
            arity: Arity::AtLeast(2),
            signal: Signal::silent(),
            category: "special form",
            example: "(defmacro my-if (cond then else) `(cond (,cond) ,then ,else))",
            aliases: &[],
        },
        // === Prelude macros (syntax sugar) ===
        Doc {
            name: "defn",
            doc: "Define a named function. Shorthand for (def name (fn (params) body...)).",
            params: &["name", "(params...)", "body..."],
            arity: Arity::AtLeast(2),
            signal: Signal::silent(),
            category: "syntax sugar",
            example: "(defn add (x y) (+ x y))",
            aliases: &[],
        },
        Doc {
            name: "let*",
            doc: "Sequential let. Each binding can reference previous bindings. Desugars to nested let.",
            params: &["((name value) ...)", "body..."],
            arity: Arity::AtLeast(1),
            signal: Signal::silent(),
            category: "syntax sugar",
            example: "(let* [x 1 y (+ x 1)] (+ x y))",
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
            example: "(->> 5 (- 10) (* 2))  # => (* (- 10 5) 2) => 10",
            aliases: &[],
        },
        Doc {
            name: "when",
            doc: "Evaluate body when condition is true. Returns nil if false.",
            params: &["condition", "body..."],
            arity: Arity::AtLeast(1),
            signal: Signal::silent(),
            category: "syntax sugar",
            example: "(when (> x 0) (display \"positive\"))",
            aliases: &[],
        },
        Doc {
            name: "unless",
            doc: "Evaluate body when condition is false. Returns nil if true.",
            params: &["condition", "body..."],
            arity: Arity::AtLeast(1),
            signal: Signal::silent(),
            category: "syntax sugar",
            example: "(unless (empty? lst) (first lst))",
            aliases: &[],
        },
        Doc {
            name: "error",
            doc: "Signal an error. The value can be anything; by convention a struct {:error :kind :message \"msg\"}. With no argument, signals nil.",
            params: &["value?"],
            arity: Arity::Range(0, 1),
            signal: Signal::yields(),
            category: "syntax sugar",
            example: "(error)\n(error {:error :not-found :message \"missing key\"})\n(error \"something broke\")",
            aliases: &[],
        },
        Doc {
            name: "try",
            doc: "Error handling. Evaluates body; if an error is signaled, evaluates catch handler with the error value.",
            params: &["body", "(catch e handler)"],
            arity: Arity::Exact(2),
            signal: Signal::silent(),
            category: "syntax sugar",
            example: "(try (/ 1 0) (catch e (display e)))",
            aliases: &[],
        },
        Doc {
            name: "protect",
            doc: "Execute body with cleanup. Cleanup runs whether body succeeds or fails.",
            params: &["body", "cleanup..."],
            arity: Arity::AtLeast(2),
            signal: Signal::silent(),
            category: "syntax sugar",
            example: "(protect (read-file f) (close f))",
            aliases: &[],
        },
        Doc {
            name: "defer",
            doc: "Register cleanup to run when the enclosing scope exits.",
            params: &["body..."],
            arity: Arity::AtLeast(1),
            signal: Signal::silent(),
            category: "syntax sugar",
            example: "(defer (close handle))",
            aliases: &[],
        },
        Doc {
            name: "with",
            doc: "Bind a resource and ensure cleanup. Combines let + protect.",
            params: &["(name init)", "body..."],
            arity: Arity::AtLeast(1),
            signal: Signal::silent(),
            category: "syntax sugar",
            example: "(with (f (open \"file.txt\")) (read-file f))",
            aliases: &[],
        },
        Doc {
            name: "yield*",
            doc: "Delegate to a sub-fiber, yielding all its values bidirectionally.",
            params: &["generator"],
            arity: Arity::Exact(1),
            signal: Signal::yields(),
            category: "syntax sugar",
            example: "(defn gen () (yield* (sub-gen)))",
            aliases: &[],
        },
        Doc {
            name: "ffi/defbind",
            doc: "Define a named wrapper for a C function via FFI. Looks up the symbol, creates a signature, and defines a function that calls it.",
            params: &["name", "lib-handle", "\"c-name\"", "return-type", "[arg-types...]"],
            arity: Arity::Exact(5),
            signal: Signal::silent(),
            category: "syntax sugar",
            example: "(ffi/defbind abs libc \"abs\" :int [:int])",
            aliases: &[],
        },
    ];

    for doc in builtins {
        docs.insert(doc.name.to_string(), doc.clone());
    }
}

/// Generate help text from the primitive definition tables.
///
/// Groups primitives by category, showing name and doc for each.
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

    out.push_str("\nSpecial forms:\n");
    out.push_str("  if, let, letrec, fn, def, var, set, begin, block, break,\n");
    out.push_str("  match, while, each, yield, and, or, quote, cond, eval, defmacro, doc,\n");
    out.push_str("  splice\n");
    out.push_str("\nSyntax sugar:\n");
    out.push_str(
        "  defn, let*, ->, ->>, when, unless, error, try, protect, defer, with, yield*,\n",
    );
    out.push_str("  ffi/defbind\n");
    out.push_str("\nREPL commands:\n");
    out.push_str("  (help)         Show this help\n");
    out.push_str("  (doc \"name\")   Show documentation for any named form\n");
    out.push_str("  (exit)         Exit the REPL\n");

    out
}
