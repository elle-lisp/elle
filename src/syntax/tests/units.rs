// audited: 2026-10-13
//! A unit mints the same scope ids and gensym names wherever it expands, and two units mint different ones.
//!
//! docs/impl/syntax.md
//! docs/macros.md

use crate::pipeline::eval_all;
use crate::runtime::Runtime;
use crate::syntax::{ScopeId, UnitTag};

/// A file unit that answers what it minted: the name `gensym` answered while
/// the unit expanded, and a syntax object for `x`, which carries the unit's
/// file scope.
pub(crate) const FILE_UNIT: &str = "(defmacro fresh [] (list 'quote (gensym \"t\")))\n[(fresh) `x]";

/// An eval of one datum that defines a gensym macro and expands it.
const EVAL_UNIT: &str = "(eval '(begin (defmacro fresh [] (list 'quote (gensym \"t\"))) (fresh)))";

/// What a unit minted, as far as the program it compiled can see.
#[derive(Debug, PartialEq)]
pub(crate) struct Minted {
    name: String,
    scopes: Vec<ScopeId>,
}

/// Compile and run `source` as the file `name` on `rt`, and read back what
/// [`FILE_UNIT`] answers.
pub(crate) fn run_file(rt: &mut Runtime, source: &str, name: &str) -> Minted {
    let (vm, symbols, cctx) = rt.parts();
    let answer = eval_all(source, symbols, vm, cctx, name).expect("the unit runs");
    let items = answer.as_array().expect("the unit answers an array");
    let id = items[0].as_symbol().expect("gensym answers a symbol");
    Minted {
        name: symbols
            .name(id)
            .expect("a gensym has a spelling")
            .to_string(),
        scopes: items[1]
            .as_syntax()
            .expect("a syntax object")
            .scopes()
            .to_vec(),
    }
}

/// Run `source` on `rt` and answer its value.
fn run(rt: &mut Runtime, source: &str) -> crate::value::Value {
    let (vm, symbols, cctx) = rt.parts();
    eval_all(source, symbols, vm, cctx, "<units>").expect("the source runs")
}

/// Run `source`, whose value is a symbol, and answer its spelling.
fn symbol_name(rt: &mut Runtime, source: &str) -> String {
    let id = run(rt, source)
        .as_symbol()
        .expect("the source answers a symbol");
    rt.symbols()
        .name(id)
        .expect("a symbol has a spelling")
        .to_string()
}

// The counter-factual is a count that runs across the process: the second
// instance's gensym carries on from the first instance's, and the two answer
// different names for one file.
#[test]
fn one_unit_mints_the_same_ids_in_two_instances() {
    let first = run_file(&mut Runtime::without_stdlib(), FILE_UNIT, "unit.lisp");
    let second = run_file(&mut Runtime::without_stdlib(), FILE_UNIT, "unit.lisp");
    assert_eq!(first, second);
}

// The trap is the VM's `eval` expander, which lives as long as the VM and
// would carry its counts from one eval into the next.
#[test]
fn one_unit_mints_the_same_ids_in_two_evals() {
    let mut rt = Runtime::without_stdlib();
    let first = symbol_name(&mut rt, EVAL_UNIT);
    let second = symbol_name(&mut rt, EVAL_UNIT);
    assert_eq!(first, second);
}

// A transformer compiles in the first unit that expands its macro, and every
// later unit calls the cached closure. `kind-of`'s body uses `case`, which
// calls `gensym` while it expands, so a transformer compile that counted in
// the caller's unit would move the caller's gensym count on the first eval
// below and not on the second.
#[test]
fn a_unit_mints_the_same_ids_whether_or_not_its_transformers_have_compiled() {
    let mut rt = Runtime::without_stdlib();
    run(
        &mut rt,
        "(eval '(defmacro kind-of [x] (case (syntax->datum x) 1 ''one ''other)))",
    );
    let unit = "(eval '(begin (kind-of 1) \
                (defmacro fresh [] (list 'quote (gensym \"t\"))) (fresh)))";
    let compiled_here = symbol_name(&mut rt, unit);
    let cached = symbol_name(&mut rt, unit);
    assert_eq!(compiled_here, cached);
}

// The tag hashes the forms as read and no span, so the same forms set out
// differently in the file are the same unit.
#[test]
fn a_units_tag_ignores_spans() {
    let mut rt = Runtime::without_stdlib();
    let spread = format!("\n\n   {}", FILE_UNIT.replace(' ', "   "));
    assert_eq!(
        run_file(&mut rt, FILE_UNIT, "unit.lisp"),
        run_file(&mut rt, &spread, "unit.lisp")
    );
}

// The counter-factual is a count with no tag: every file's scope would be the
// first count of its unit, and a syntax object one file wrote would carry the
// file scope of any other.
#[test]
fn two_units_with_different_forms_take_different_tags() {
    let mut rt = Runtime::without_stdlib();
    let first = run_file(&mut rt, FILE_UNIT, "unit.lisp");
    let second = run_file(&mut rt, &format!("(def other 1)\n{FILE_UNIT}"), "unit.lisp");
    assert_ne!(first.scopes, second.scopes);
    assert_ne!(first.name, second.name);
}

#[test]
fn two_units_with_different_names_take_different_tags() {
    let mut rt = Runtime::without_stdlib();
    let first = run_file(&mut rt, FILE_UNIT, "one.lisp");
    let second = run_file(&mut rt, FILE_UNIT, "two.lisp");
    assert_ne!(first.scopes, second.scopes);
    assert_ne!(first.name, second.name);
}

// The widest count under the widest tag: a count that spilled into the intro
// bit, or an intro bit that spilled into the tag, would show here.
#[test]
fn an_id_keeps_its_tag_its_intro_bit_and_its_count_apart() {
    let widest = (1u32 << 31) - 1;
    let tagged = |id: ScopeId| ScopeId::from_bits((0xffff_ffffu64 << 32) | id.to_bits());
    let plain = tagged(ScopeId::new(UnitTag::NONE, widest));
    let intro = tagged(ScopeId::intro(UnitTag::NONE, widest));
    assert!(!plain.is_intro());
    assert!(intro.is_intro());
    assert_eq!(plain.count(), widest);
    assert_eq!(intro.count(), widest);
    assert_ne!(plain, ScopeId::new(UnitTag::NONE, widest));
    assert_ne!(
        intro,
        ScopeId::COMPILER,
        "a unit's widest intro scope is the compiler's"
    );
}

// The file scope is the first scope the unit mints, so its count is zero
// whatever the instance compiled before.
#[test]
fn a_units_count_starts_at_zero() {
    let mut rt = Runtime::without_stdlib();
    run_file(&mut rt, "(def before 1)\n[(gensym) `x]", "before.lisp");
    let scopes = run_file(&mut rt, FILE_UNIT, "unit.lisp").scopes;
    assert_eq!(scopes.len(), 1, "{scopes:?}");
    assert_eq!(scopes[0].count(), 0, "{scopes:?}");
    assert!(!scopes[0].is_intro(), "{scopes:?}");
}

// While an expansion runs, gensym answers its prefix, the unit's count, a
// hyphen, and the unit's tag in eight lower-case hex digits. The file unit
// calls gensym once, so its count is zero.
#[test]
fn an_expansion_time_gensym_names_its_unit() {
    let name = run_file(&mut Runtime::without_stdlib(), FILE_UNIT, "unit.lisp").name;
    let (count, tag) = name
        .strip_prefix('t')
        .and_then(|rest| rest.split_once('-'))
        .unwrap_or_else(|| panic!("{name} does not name a unit"));
    assert_eq!(count, "0", "{name}");
    assert_eq!(tag.len(), 8, "{name}");
    assert!(
        tag.chars()
            .all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c)),
        "{name}"
    );
}
