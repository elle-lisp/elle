// The post-boot heap census (docs/impl/image.md § "Open risks and dispatch
// experiments", item 2): after `Runtime::new()` completes, every live object
// in the instance's region store is boot state, and the census enumerates the
// graph a boot image must dump. These tests are the permanent regression net
// for the sealing claims: they fail the moment an unsealed variant enters the
// boot graph, which is exactly the condition that would make the boot image
// undumpable.

use elle::runtime::Runtime;
use elle::value::fiberheap::census::Sealing;
use elle::value::HeapTag;
use std::process::Command;

// ── The sealing net ─────────────────────────────────────────────────

// docs/impl/image.md § Sealing names the boot graph's only unsealed leaves,
// both handled by the reconstruction stream: the three `External` stdio
// ports (the *stdin*/*stdout*/*stderr* `Parameter` defaults) and the
// instance's two default traitsets (`@struct`s built by `init_default_traits`
// at VM init). Anything else unsealed is a new dump obstacle and must be
// recorded in the design before this expectation moves. The counter-factual
// this guards: a stdlib change that defines a mutable top-level (an @struct
// cache, a box) would boot fine and pass every behavioral test, yet silently
// make the future boot image undumpable — this assertion is what fails
// instead.
#[test]
fn boot_heap_unsealed_leaves_are_exactly_the_reconstructible_set() {
    let mut rt = Runtime::new();
    let census = rt.heap().census();

    let mut unsealed: Vec<String> = census
        .unsealed
        .iter()
        .map(|leaf| format!("{:?}({}) x{}", leaf.tag, leaf.detail, leaf.count))
        .collect();
    unsealed.sort();
    assert_eq!(
        unsealed,
        vec![
            "External(port) x3".to_string(),
            "LStructMut() x2".to_string()
        ],
        "boot graph's unsealed leaves changed; census:\n{}",
        census.lines().join("\n")
    );
}

// § Sealing: "The stdlib file-letrec allocates one `CaptureCell` per captured
// top-level binding" — the cells snapping must rewrite. Zero cells would mean
// the letrec lowering changed shape and the snapping design step is stale.
#[test]
fn boot_heap_holds_capture_cells_for_snapping() {
    let mut rt = Runtime::new();
    let census = rt.heap().census();
    assert!(
        census.capture_cells > 0,
        "expected stdlib letrec capture cells in the boot graph; census:\n{}",
        census.lines().join("\n")
    );
}

// Shape of the census itself: the boot graph must show closures, region
// templates, pointer slots, and region slices, and the store must hold the
// many per-cell letrec regions the design's compaction argument rests on.
#[test]
fn boot_census_reports_the_expected_shape() {
    let mut rt = Runtime::new();
    let census = rt.heap().census();

    assert!(census.objects > 0, "empty boot census");
    assert!(
        census.regions > 1,
        "boot produced {} region(s); the compaction argument expects many",
        census.regions
    );
    // The trap this loop already caught once: the boot residue is function
    // definitions, not data — no `Pair` (or string, or array) survives to
    // the post-boot heap, so only the code-object tags may be asserted here.
    for tag in [
        HeapTag::Closure,
        HeapTag::ClosureTemplate,
        HeapTag::CaptureCell,
    ] {
        assert!(
            census.tags.iter().any(|t| t.tag == tag && t.count > 0),
            "boot census lacks {:?} objects:\n{}",
            tag,
            census.lines().join("\n")
        );
    }
    assert!(census.ptr_slots > 0, "no pointer slots counted");
    assert!(census.region_slices > 0, "no region slices counted");
    // The per-tag rows must add up to the totals the density math uses.
    let count_sum: usize = census.tags.iter().map(|t| t.count).sum();
    assert_eq!(count_sum, census.objects, "per-tag counts drift from total");
}

// The sealing classification is the image design's sealed set (§ Sealing),
// not an implementation echo: every mutable variant and every process/foreign
// handle is refused, CaptureCell is snapped, everything else is body data.
#[test]
fn sealing_classification_matches_the_design() {
    use elle::value::fiberheap::census::sealing;
    for tag in [
        HeapTag::LString,
        HeapTag::Pair,
        HeapTag::LStruct,
        HeapTag::Closure,
        HeapTag::LArray,
        HeapTag::LBytes,
        HeapTag::LSet,
        HeapTag::Syntax,
        HeapTag::Float,
        HeapTag::Parameter,
        HeapTag::ClosureTemplate,
    ] {
        assert_eq!(sealing(tag), Sealing::Sealed, "{:?} should be sealed", tag);
    }
    assert_eq!(sealing(HeapTag::CaptureCell), Sealing::Snapped);
    for tag in [
        HeapTag::LArrayMut,
        HeapTag::LStructMut,
        HeapTag::LStringMut,
        HeapTag::LBytesMut,
        HeapTag::LSetMut,
        HeapTag::LBox,
        HeapTag::LibHandle,
        HeapTag::ThreadHandle,
        HeapTag::Fiber,
        HeapTag::FFISignature,
        HeapTag::FFIType,
        HeapTag::ManagedPointer,
        HeapTag::External,
    ] {
        assert_eq!(
            sealing(tag),
            Sealing::Unsealed,
            "{:?} should be unsealed",
            tag
        );
    }
}

// ── The CLI diagnostic ──────────────────────────────────────────────

// `--trace=` rejects unknown keywords, so a binary without the `census`
// keyword fails this run outright rather than running silently without the
// report (the same trap trace_boot.rs pins for `boot`).
#[test]
fn census_trace_reports_the_boot_heap() {
    let dir = crate::common::ScratchDir::new("trace-census");
    let script = dir.join("script.lisp");
    std::fs::write(&script, "(+ 1 2)\n").expect("write script");
    let out = Command::new(env!("CARGO_BIN_EXE_elle"))
        .arg("--trace=census")
        .arg(&script)
        .output()
        .expect("run elle");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        out.status.success(),
        "elle --trace=census failed; stderr:\n{}",
        stderr
    );
    for needle in [
        "[trace:census] regions ",
        "[trace:census] tag ",
        "[trace:census] capture-cells ",
        "[trace:census] unsealed",
    ] {
        assert!(
            stderr.lines().any(|l| l.starts_with(needle)),
            "missing '{}' line; stderr:\n{}",
            needle,
            stderr
        );
    }
}
