//! Built-in documentation for special forms and prelude macros.

use super::def::Doc;

/// Register documentation for special forms and prelude macros.
///
/// These aren't primitives (no NativeFn) but they should be discoverable
/// via `(doc "if")`, `(doc "defn")`, etc. Called during `register_primitives`.
pub(crate) fn register_builtin_docs(docs: &mut std::collections::HashMap<String, Doc>) {
    use crate::effects::Effect;
    use crate::value::types::Arity;

    let builtins: &[Doc] = &[
        // === Special forms ===
        Doc {
            name: "if",
            doc: "Conditional expression. Evaluates condition, then either the then-branch or the else-branch.",
            params: &["condition", "then", "else?"],
            arity: Arity::Range(2, 3),
            effect: Effect::none(),
            category: "special form",
            example: "(if (> x 0) \"positive\" \"non-positive\")",
            aliases: &[],
        },
        Doc {
            name: "let",
            doc: "Bind values to names in a new scope. Supports destructuring patterns.",
            params: &["((name value) ...)", "body..."],
            arity: Arity::AtLeast(1),
            effect: Effect::none(),
            category: "special form",
            example: "(let ((x 1) (y 2)) (+ x y))",
            aliases: &[],
        },
        Doc {
            name: "letrec",
            doc: "Recursive let. Bindings can reference each other (for mutual recursion).",
            params: &["((name value) ...)", "body..."],
            arity: Arity::AtLeast(1),
            effect: Effect::none(),
            category: "special form",
            example: "(letrec ((even? (fn (n) (if (= n 0) true (odd? (- n 1))))) (odd? (fn (n) (if (= n 0) false (even? (- n 1)))))) (even? 10))",
            aliases: &[],
        },
        Doc {
            name: "fn",
            doc: "Create an anonymous function (lambda). Supports destructuring in parameters.",
            params: &["(params...)", "body..."],
            arity: Arity::AtLeast(1),
            effect: Effect::none(),
            category: "special form",
            example: "(fn (x y) (+ x y))",
            aliases: &[],
        },
        Doc {
            name: "def",
            doc: "Bind a value to an immutable name. Supports destructuring patterns including lists, arrays, and tables.",
            params: &["pattern", "value"],
            arity: Arity::Exact(2),
            effect: Effect::none(),
            category: "special form",
            example: "(def x 42)\n(def {:name n :age a} {:name \"Alice\" :age 30})",
            aliases: &[],
        },
        Doc {
            name: "var",
            doc: "Bind a value to a mutable name. Supports destructuring. Use set! to mutate.",
            params: &["pattern", "value"],
            arity: Arity::Exact(2),
            effect: Effect::none(),
            category: "special form",
            example: "(var x 0)\n(set! x (+ x 1))",
            aliases: &[],
        },
        Doc {
            name: "set!",
            doc: "Mutate a var binding. Only works on names defined with var.",
            params: &["name", "value"],
            arity: Arity::Exact(2),
            effect: Effect::none(),
            category: "special form",
            example: "(var x 0) (set! x 42)",
            aliases: &[],
        },
        Doc {
            name: "begin",
            doc: "Sequence expressions. Does NOT create a scope — bindings leak into the enclosing scope.",
            params: &["expr..."],
            arity: Arity::AtLeast(0),
            effect: Effect::none(),
            category: "special form",
            example: "(begin (def x 1) (def y 2) (+ x y))",
            aliases: &[],
        },
        Doc {
            name: "block",
            doc: "Sequence expressions in a new lexical scope. Supports optional keyword name for break targeting.",
            params: &[":name?", "body..."],
            arity: Arity::AtLeast(0),
            effect: Effect::none(),
            category: "special form",
            example: "(block :outer (if done (break :outer result)) (continue))",
            aliases: &[],
        },
        Doc {
            name: "break",
            doc: "Exit a named block with a value. Must be inside a block; cannot cross function boundaries.",
            params: &[":name?", "value"],
            arity: Arity::Range(1, 2),
            effect: Effect::none(),
            category: "special form",
            example: "(block :loop (break :loop 42))",
            aliases: &[],
        },
        Doc {
            name: "match",
            doc: "Pattern matching. Tests value against patterns in order, executing the first matching arm.",
            params: &["value", "(pattern body)..."],
            arity: Arity::AtLeast(2),
            effect: Effect::none(),
            category: "special form",
            example: "(match x (0 \"zero\") ((a . b) (+ a b)) (_ \"other\"))",
            aliases: &[],
        },
        Doc {
            name: "while",
            doc: "Loop while condition is true. Returns nil.",
            params: &["condition", "body..."],
            arity: Arity::AtLeast(1),
            effect: Effect::none(),
            category: "special form",
            example: "(var i 0) (while (< i 10) (set! i (+ i 1)))",
            aliases: &[],
        },
        Doc {
            name: "each",
            doc: "Iterate over a list, binding each element to a name.",
            params: &["(name list)", "body..."],
            arity: Arity::AtLeast(1),
            effect: Effect::none(),
            category: "special form",
            example: "(each (x (list 1 2 3)) (display x))",
            aliases: &[],
        },
        Doc {
            name: "yield",
            doc: "Yield a value from a coroutine/fiber. Suspends execution until resumed.",
            params: &["value"],
            arity: Arity::Exact(1),
            effect: Effect::yields(),
            category: "special form",
            example: "(fn () (yield 1) (yield 2) (yield 3))",
            aliases: &[],
        },
        Doc {
            name: "and",
            doc: "Short-circuit logical AND. Returns the first falsy value, or the last value if all truthy.",
            params: &["expr..."],
            arity: Arity::AtLeast(0),
            effect: Effect::none(),
            category: "special form",
            example: "(and (> x 0) (< x 100))",
            aliases: &[],
        },
        Doc {
            name: "or",
            doc: "Short-circuit logical OR. Returns the first truthy value, or the last value if all falsy.",
            params: &["expr..."],
            arity: Arity::AtLeast(0),
            effect: Effect::none(),
            category: "special form",
            example: "(or default-value (compute-value))",
            aliases: &[],
        },
        Doc {
            name: "quote",
            doc: "Return the unevaluated form. Prevents evaluation of its argument.",
            params: &["form"],
            arity: Arity::Exact(1),
            effect: Effect::none(),
            category: "special form",
            example: "(quote (+ 1 2))  ; => (+ 1 2)",
            aliases: &[],
        },
        Doc {
            name: "cond",
            doc: "Multi-branch conditional. Tests clauses in order, evaluating the body of the first true test.",
            params: &["(test body)..."],
            arity: Arity::AtLeast(1),
            effect: Effect::none(),
            category: "special form",
            example: "(cond ((< x 0) \"negative\") ((= x 0) \"zero\") (true \"positive\"))",
            aliases: &[],
        },
        Doc {
            name: "module",
            doc: "Define a module with exported bindings.",
            params: &["name", "body..."],
            arity: Arity::AtLeast(1),
            effect: Effect::none(),
            category: "special form",
            example: "(module math (def pi 3.14159) (defn square (x) (* x x)))",
            aliases: &[],
        },
        Doc {
            name: "import",
            doc: "Import bindings from a module.",
            params: &["module-name"],
            arity: Arity::Exact(1),
            effect: Effect::none(),
            category: "special form",
            example: "(import math)",
            aliases: &[],
        },
        Doc {
            name: "defmacro",
            doc: "Define a syntax macro. The macro function receives syntax objects and returns a syntax object.",
            params: &["name", "(params...)", "body..."],
            arity: Arity::AtLeast(2),
            effect: Effect::none(),
            category: "special form",
            example: "(defmacro my-if (cond then else) `(cond ((,cond) ,then) (true ,else)))",
            aliases: &[],
        },
        // === Prelude macros (syntax sugar) ===
        Doc {
            name: "defn",
            doc: "Define a named function. Shorthand for (def name (fn (params) body...)).",
            params: &["name", "(params...)", "body..."],
            arity: Arity::AtLeast(2),
            effect: Effect::none(),
            category: "syntax sugar",
            example: "(defn add (x y) (+ x y))",
            aliases: &[],
        },
        Doc {
            name: "let*",
            doc: "Sequential let. Each binding can reference previous bindings. Desugars to nested let.",
            params: &["((name value) ...)", "body..."],
            arity: Arity::AtLeast(1),
            effect: Effect::none(),
            category: "syntax sugar",
            example: "(let* ((x 1) (y (+ x 1))) (+ x y))",
            aliases: &[],
        },
        Doc {
            name: "->",
            doc: "Thread-first macro. Inserts value as first argument of each successive form.",
            params: &["value", "forms..."],
            arity: Arity::AtLeast(1),
            effect: Effect::none(),
            category: "syntax sugar",
            example: "(-> 5 (+ 3) (* 2))  ; => (* (+ 5 3) 2) => 16",
            aliases: &[],
        },
        Doc {
            name: "->>",
            doc: "Thread-last macro. Inserts value as last argument of each successive form.",
            params: &["value", "forms..."],
            arity: Arity::AtLeast(1),
            effect: Effect::none(),
            category: "syntax sugar",
            example: "(->> 5 (- 10) (* 2))  ; => (* (- 10 5) 2) => 10",
            aliases: &[],
        },
        Doc {
            name: "when",
            doc: "Evaluate body when condition is true. Returns nil if false.",
            params: &["condition", "body..."],
            arity: Arity::AtLeast(1),
            effect: Effect::none(),
            category: "syntax sugar",
            example: "(when (> x 0) (display \"positive\"))",
            aliases: &[],
        },
        Doc {
            name: "unless",
            doc: "Evaluate body when condition is false. Returns nil if true.",
            params: &["condition", "body..."],
            arity: Arity::AtLeast(1),
            effect: Effect::none(),
            category: "syntax sugar",
            example: "(unless (empty? lst) (first lst))",
            aliases: &[],
        },
        Doc {
            name: "try",
            doc: "Error handling. Evaluates body; if an error is signaled, evaluates catch handler with the error value.",
            params: &["body", "(catch (e) handler)"],
            arity: Arity::Exact(2),
            effect: Effect::none(),
            category: "syntax sugar",
            example: "(try (/ 1 0) (catch (e) (display e)))",
            aliases: &[],
        },
        Doc {
            name: "protect",
            doc: "Execute body with cleanup. Cleanup runs whether body succeeds or fails.",
            params: &["body", "cleanup..."],
            arity: Arity::AtLeast(2),
            effect: Effect::none(),
            category: "syntax sugar",
            example: "(protect (read-file f) (close f))",
            aliases: &[],
        },
        Doc {
            name: "defer",
            doc: "Register cleanup to run when the enclosing scope exits.",
            params: &["body..."],
            arity: Arity::AtLeast(1),
            effect: Effect::none(),
            category: "syntax sugar",
            example: "(defer (close handle))",
            aliases: &[],
        },
        Doc {
            name: "with",
            doc: "Bind a resource and ensure cleanup. Combines let + protect.",
            params: &["(name init)", "body..."],
            arity: Arity::AtLeast(1),
            effect: Effect::none(),
            category: "syntax sugar",
            example: "(with (f (open \"file.txt\")) (read-file f))",
            aliases: &[],
        },
        Doc {
            name: "yield*",
            doc: "Delegate to a sub-coroutine, yielding all its values bidirectionally.",
            params: &["generator"],
            arity: Arity::Exact(1),
            effect: Effect::yields(),
            category: "syntax sugar",
            example: "(defn gen () (yield* (sub-gen)))",
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
    out.push_str("  if, let, def, var, fn, set!, begin, block, break,\n");
    out.push_str("  match, while, each, yield, quote, defmacro, module, import\n");
    out.push_str("\nSyntax sugar:\n");
    out.push_str("  defn, let*, ->, ->>, when, unless, try/catch, protect, defer, with\n");
    out.push_str("\nREPL commands:\n");
    out.push_str("  (help)         Show this help\n");
    out.push_str("  (doc \"name\")   Show documentation for any named form\n");
    out.push_str("  (exit)         Exit the REPL\n");

    out
}
