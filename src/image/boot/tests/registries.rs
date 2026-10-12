// audited: 2026-10-12
//! Pins the compile registries a boot image carries: what an image boot restores, the rewrites they drive, and what refuses.
//!
//! docs/impl/image/boot.md

use std::collections::{BTreeMap, BTreeSet};

use crate::hir::{BindingArena, Hir, HirKind};
use crate::image::ImageError;
use crate::runtime::Runtime;
use crate::symbol::SymbolTable;
use crate::value::{build, TableKey, Value};

use super::super::compiler::Registries;
use super::super::BootSource;
use super::Scratch;

/// A runtime that booted from source over `scratch`, which stores the image,
/// and a second one that hydrated it.
fn both_boots(scratch: &Scratch) -> (Runtime, Runtime) {
    let source = Runtime::with_caches(scratch.caches());
    assert_eq!(source.boot_source(), BootSource::Compiled, "no source boot");
    let image = Runtime::with_caches(scratch.caches());
    assert_eq!(image.boot_source(), BootSource::Image, "no image boot");
    (source, image)
}

/// One wrapper's arity and arms, each arm as its container type and the op it
/// routes to: everything a later compile's rewrite reads off an entry.
type WrapperShape = (usize, Vec<(u32, String)>);

/// The dispatch-wrapper registry by name, and the inline registry's names.
fn registries(rt: &mut Runtime) -> (BTreeMap<String, WrapperShape>, BTreeSet<String>) {
    let (_vm, symbols, cctx) = rt.parts();
    let name = |id| symbols.name(id).unwrap_or("?").to_string();
    let (wrappers, inline) = cctx.compile_registries_mut();
    let wrappers = wrappers
        .by_name
        .iter()
        .map(|(id, w)| {
            let arms = w
                .arms
                .iter()
                .map(|a| (a.ty.0, name(a.native_name)))
                .collect();
            (name(*id), (w.arity, arms))
        })
        .collect();
    let inline = inline.by_name.keys().map(|id| name(*id)).collect();
    (wrappers, inline)
}

/// The name of every callee `src` calls once it compiles in `rt`, read off the
/// regularized HIR: a fused `map` and a monomorphized `push` have each
/// rewritten their call away by then.
fn callees(rt: &mut Runtime, src: &str) -> Vec<String> {
    fn walk(h: &Hir, arena: &BindingArena, symbols: &SymbolTable, out: &mut Vec<String>) {
        if let HirKind::Call { func, .. } = &h.kind {
            let callee = match &func.kind {
                HirKind::Var(b) => Some(*b),
                HirKind::DerefCell { cell } => match &cell.kind {
                    HirKind::Var(b) => Some(*b),
                    _ => None,
                },
                _ => None,
            };
            if let Some(b) = callee {
                out.push(symbols.name(arena.get(b).name).unwrap_or("?").to_string());
            }
        }
        h.for_each_child(|c| walk(c, arena, symbols, out));
    }

    let (_vm, symbols, cctx) = rt.parts();
    let (hir, arena) = crate::pipeline::compile_file_to_fhir(src, symbols, cctx, "<registries>")
        .expect("compiles to HIR");
    let mut out = Vec::new();
    walk(&hir, &arena, symbols, &mut out);
    out
}

// § "The compile registries cross as bytes": an image boot holds every entry
// the source boot it was dumped from recorded, wrapper arms included.
//
// The counter-factual is an empty registry. It compiles every program
// correctly, so nothing but the registry itself shows the loss.
#[test]
fn an_image_boot_restores_both_compile_registries() {
    let scratch = Scratch::new("registries");
    let (mut source, mut image) = both_boots(&scratch);
    let (wrappers, inline) = registries(&mut source);
    assert!(
        !wrappers.is_empty() && !inline.is_empty(),
        "a source boot recorded no entries, so this test exercises nothing"
    );
    let (hydrated_wrappers, hydrated_inline) = registries(&mut image);
    assert_eq!(
        hydrated_wrappers, wrappers,
        "the image boot's dispatch wrappers differ from the source boot's"
    );
    assert_eq!(
        hydrated_inline, inline,
        "the image boot's inline fragments differ from the source boot's"
    );
}

// What the inline registry buys: a user `map` over a stdlib function fuses
// into a loop, and no call to `map` is left. `inc` closes over a global and
// `fn/cfg-label` over a `let`, the two shapes a fragment takes.
//
// The trap is the callee to look for. An unfused `(map inc xs)` passes `inc`
// as an argument and never calls it, so the absence of a call to `inc` holds
// whether or not the map fused. The call that fusion removes is the one to
// `map`.
//
// Two boots never emit byte-identical bytecode for one form, because operands
// carry ids from a process-global mint counter that a source boot advances and
// an image boot does not. So the claim is asserted in the rewritten HIR.
#[test]
fn a_user_map_over_a_stdlib_function_fuses_under_either_boot() {
    const CASES: [&str; 2] = [
        "(map inc [1 2 3])",
        r#"(map fn/cfg-label [{:name "a"} {:name "b"}])"#,
    ];
    let scratch = Scratch::new("registries-fuse");
    let (mut source, mut image) = both_boots(&scratch);
    for src in CASES {
        for (boot, rt) in [
            ("a source boot", &mut source),
            ("an image boot", &mut image),
        ] {
            let calls = callees(rt, src);
            assert!(
                !calls.iter().any(|c| c == "map"),
                "{boot} leaves the call to map in {src}; callees were {calls:?}"
            );
        }
    }
}

// What the dispatch-wrapper registry buys: a user `push` on a proven array
// collapses to its monomorphic op under either boot.
#[test]
fn a_user_push_monomorphizes_under_either_boot() {
    const SRC: &str = "(push [1 2] 3)";
    let scratch = Scratch::new("registries-push");
    let (mut source, mut image) = both_boots(&scratch);
    for (boot, rt) in [
        ("a source boot", &mut source),
        ("an image boot", &mut image),
    ] {
        let calls = callees(rt, SRC);
        assert!(
            calls.iter().any(|c| c == "%push-array") && !calls.iter().any(|c| c == "push"),
            "{boot} does not collapse {SRC} to %push-array; callees were {calls:?}"
        );
    }
}

// § "The compile registries cross as bytes": the encoded entries are in name
// order, so one boot state writes one file whatever order the registries' hash
// maps iterate in.
//
// The counter-factual is an encoding in iteration order. It round-trips every
// entry, and two dumps from one instance still agree, because one map iterates
// one way twice. Only a second process, whose maps seed differently, would
// write another file.
#[test]
fn the_encoded_registries_are_in_name_order() {
    let mut rt = Runtime::with_stdlib_cache(crate::compiler::stdlib_cache::StdlibCache::Off);
    let (wrappers, inline) = registries(&mut rt);
    let (_vm, symbols, cctx) = rt.parts();
    let decoded = Registries::decode(&Registries::of(cctx, symbols).encode()).expect("decodes");

    let wrapper_names: Vec<&str> = decoded
        .dispatch_wrappers
        .by_name
        .iter()
        .map(|(n, _)| n.as_str())
        .collect();
    let inline_names: Vec<&str> = decoded
        .fn_inline
        .by_name
        .iter()
        .map(|(n, _)| n.as_str())
        .collect();
    assert_eq!(
        wrapper_names,
        wrappers.keys().map(String::as_str).collect::<Vec<_>>(),
        "the encoded dispatch wrappers are not every wrapper, in name order"
    );
    assert_eq!(
        inline_names,
        inline.iter().map(String::as_str).collect::<Vec<_>>(),
        "the encoded inline fragments are not every fragment, in name order"
    );
}

// § "The compile registries cross as bytes": a `:compiler` field that is
// absent, is not bytes, or does not decode refuses the image as corrupt. Each
// image here is the stored one with only that field changed, so its digest and
// its fingerprint still pass.
//
// The counter-factual is an install that skips a bad field. The instance then
// boots without the registries, which compiles every program correctly and
// worse, and reports an image boot.
#[test]
fn a_compiler_field_that_does_not_decode_refuses_the_image() {
    let scratch = Scratch::new("registries-corrupt");
    drop(Runtime::with_caches(scratch.caches()));
    let stored = scratch.image();

    let mut probe = Runtime::without_stdlib();
    let (heap, symbols) = probe.heap_and_symbols();
    let hydrated = crate::image::hydrate_path(heap, symbols, &stored).expect("hydrate");
    let fields: BTreeMap<TableKey, Value> = hydrated
        .root
        .as_struct()
        .expect("the root is a struct")
        .iter()
        .copied()
        .collect();
    let key = TableKey::keyword("compiler");
    symbols.keyword("compiler");

    let region = heap.new_runtime_region();
    let cases: [(&str, Option<Value>); 3] = [
        ("absent", None),
        ("a string", Some(build::string(heap, "registries", region))),
        (
            "undecodable bytes",
            Some(build::bytes(heap, vec![0xff; 3], region)),
        ),
    ];
    let mut paths = Vec::new();
    for (what, value) in cases {
        let mut root = fields.clone();
        match value {
            Some(v) => root.insert(key, v),
            None => root.remove(&key),
        };
        let root = build::struct_from(heap, root, region);
        let path = scratch.0.join(format!("{}.image", what.replace(' ', "-")));
        crate::image::dump(heap, symbols, root, &path).expect("dump");
        paths.push((what, path));
    }
    heap.decref_region_if_present(region);
    heap.decref_region_if_present(hydrated.region);

    for (what, path) in paths {
        let mut rt = Runtime::without_stdlib();
        let (heap, symbols) = rt.heap_and_symbols();
        match super::super::hydrate_path(heap, symbols, &path) {
            Err(ImageError::Corrupt(_)) => {}
            Err(e) => {
                panic!("a :compiler field that is {what} was refused for another reason: {e}")
            }
            Ok(_) => panic!("a :compiler field that is {what} was not refused"),
        }
    }
}
