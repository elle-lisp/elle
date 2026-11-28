// audited: 2026-11-24
// A quoting form means one thing in either spelling, over generated forms.
//
// docs/syntax.md
//
// A quoted form survives `eval` as the data it quotes, and `expand-macro`
// answers alike for `'form` and `(quote form)`. The generated forms mix both
// spellings of every quoting form with macro calls. The counter-factual: an
// expander that reads only the prefix node expands a macro call inside a
// quote that arrives as a list.

use crate::common::eval_reuse;
use elle::Value;
use proptest::prelude::*;

/// Source text for one form, `depth` levels deep. A symbol carries the `s-`
/// prefix, so none names a macro or a special form, and every macro call
/// has the arity its macro takes.
fn arb_form(depth: u32) -> BoxedStrategy<String> {
    let leaf = prop_oneof![
        (-100i64..100).prop_map(|n| n.to_string()),
        "s-[a-z0-9]{0,5}",
        "[a-z]{1,5}".prop_map(|s| format!(":{s}")),
        "[a-z ]{0,8}".prop_map(|s| format!("\"{s}\"")),
    ]
    .boxed();
    if depth == 0 {
        return leaf;
    }
    let inner = arb_form(depth - 1);
    let quoting = prop_oneof![
        Just(("'", "quote")),
        Just(("`", "quasiquote")),
        Just((",", "unquote")),
        Just((",;", "unquote-splicing")),
    ];
    prop_oneof![
        4 => leaf,
        2 => prop::collection::vec(inner.clone(), 0..=3)
            .prop_map(|items| format!("(s-f {})", items.join(" "))),
        1 => prop::collection::vec(inner.clone(), 0..=3)
            .prop_map(|items| format!("[{}]", items.join(" "))),
        2 => (quoting, inner.clone(), prop::bool::ANY).prop_map(|((prefix, name), form, spelled)| {
            if spelled { format!("({name} {form})") } else { format!("{prefix}{form}") }
        }),
        1 => (inner.clone(), inner).prop_map(|(a, b)| format!("(when {a} {b})")),
    ]
    .boxed()
}

proptest! {
    #![proptest_config(crate::common::proptest_cases(50))]

    #[test]
    fn a_quoted_form_survives_eval_as_its_data(form in arb_form(3)) {
        let src = format!("(= (eval '(quote {form})) '{form})");
        prop_assert_eq!(eval_reuse(&src), Ok(Value::bool(true)), "{}", src);
    }

    #[test]
    fn expand_macro_answers_alike_for_either_spelling(form in arb_form(3)) {
        let src = format!("(= (expand-macro '{form}) (expand-macro (quote {form})))");
        prop_assert_eq!(eval_reuse(&src), Ok(Value::bool(true)), "{}", src);
    }
}
