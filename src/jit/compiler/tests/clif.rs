// audited: 2026-09-05
// docs/impl/jit.md
//! What only the rendered Cranelift IR settles: the flags a load carries, and
//! the pop that precedes every exit.

use super::*;

/// fn() -> capture 0. With `num_captures = 1`, `LoadCapture` index 0 reads
/// through the closure environment pointer rather than an argument variable.
fn make_capture_read_lir() -> LirFunction {
    LirFixture::new(Arity::Exact(0))
        .signal(Signal::silent())
        .num_captures(1)
        .block(
            0,
            vec![LirInstr::LoadCapture {
                dst: Reg(0),
                index: 0,
            }],
            Terminator::Return(Reg(0)),
        )
        .build()
}

/// The `load` lines of a rendered Cranelift function, in emission order.
fn load_lines(clif: &[String]) -> Vec<&str> {
    clif.iter()
        .map(|line| line.trim())
        .filter(|line| line.contains("= load."))
        .collect()
}

#[test]
fn an_argument_load_carries_trusted_flags() {
    // Trap: memory flags change the access the backend emits, never the value
    // it computes, so nothing but the rendered CLIF shows which flags a load
    // actually got.
    //
    // Counter-factual: passing `MemFlagsData::new()` instead of `::trusted()`
    // compiles, and the compiled code returns the right answers, because the
    // argument array really is aligned and mapped. It costs a trapping,
    // unaligned-tolerant access on every parameter of every hot function.
    let compiler = JitCompiler::new().expect("Failed to create compiler");
    let clif = compiler
        .clif_text(&make_simple_lir(), None)
        .expect("Failed to translate");
    let loads = load_lines(&clif);
    assert!(
        !loads.is_empty(),
        "a one-parameter function loads its argument; got:\n{}",
        clif.join("\n")
    );
    for load in &loads {
        assert!(
            load.contains("notrap aligned"),
            "argument load without trusted flags: {load}"
        );
    }
}

#[test]
fn a_capture_load_carries_trusted_flags() {
    // The environment pointer is a second base pointer, reached from a
    // different translator path than the argument array.
    let compiler = JitCompiler::new().expect("Failed to create compiler");
    let clif = compiler
        .clif_text(&make_capture_read_lir(), None)
        .expect("Failed to translate");
    let loads = load_lines(&clif);
    assert!(
        !loads.is_empty(),
        "reading capture 0 loads from the env pointer; got:\n{}",
        clif.join("\n")
    );
    for load in &loads {
        assert!(
            load.contains("notrap aligned"),
            "capture load without trusted flags: {load}"
        );
    }
}

/// fn(f) -> f(). A `Call` inside a function whose signal may suspend, which is
/// what makes the translator emit all three exits: the post-call error check,
/// the post-call yield check, and the normal return.
fn make_suspending_call_lir() -> LirFunction {
    use crate::hir::region::StaticRegion;
    use crate::lir::CallSiteInfo;
    LirFixture::new(Arity::Exact(1))
        .signal(Signal::yields())
        .call_sites(vec![CallSiteInfo {
            resume_ip: 0,
            stack_regs: vec![],
            num_locals: 0,
        }])
        .block(
            0,
            vec![
                LirInstr::LoadCapture {
                    dst: Reg(0),
                    index: 0,
                },
                LirInstr::Call {
                    dst: Reg(1),
                    func: Reg(0),
                    args: vec![],
                    arity_checked: false,
                    region: StaticRegion::new(1).unwrap(),
                },
            ],
            Terminator::Return(Reg(1)),
        )
        .build()
}

/// `fnN` → the module function id it names, read off a rendered function's
/// preamble lines (`fn3 = u0:87 sig3`, optionally `colocated`).
fn func_refs(clif: &[String]) -> HashMap<String, u32> {
    let mut refs = HashMap::new();
    for line in clif {
        let line = line.trim();
        let Some((name, rest)) = line.split_once(" = ") else {
            continue;
        };
        if !name.starts_with("fn") {
            continue;
        }
        let Some(id) = rest
            .split_whitespace()
            .find_map(|tok| tok.strip_prefix("u0:"))
        else {
            continue;
        };
        if let Ok(id) = id.parse::<u32>() {
            refs.insert(name.to_string(), id);
        }
    }
    refs
}

/// The `fnN` of the call instruction nearest above `at`, searching back only
/// within `at`'s own block.
fn call_target_before(clif: &[String], at: usize) -> Option<String> {
    for line in clif[..at].iter().rev() {
        let line = line.trim();
        if line.starts_with("block") {
            return None;
        }
        let Some(pos) = line.find("call ") else {
            continue;
        };
        let rest = &line[pos + "call ".len()..];
        let name = rest.split('(').next()?.trim();
        return Some(name.to_string());
    }
    None
}

/// Every `return` a compiled function emits is preceded by the call that pops
/// this activation's region-remap frame, so the prologue's push is balanced on
/// every path out (docs/impl/region/mechanism.md § "An abandoned frame runs the
/// releases it still owes").
#[test]
fn every_compiled_exit_pops_the_region_map() {
    // Trap: a missing pop is invisible to the compiled function itself. It
    // returns the right value; what it leaves behind is a map frame that
    // `last()` then names for the INTERPRETER activation above it, which parks
    // and releases against a frame that was never its own — and the remap stack
    // never shrinks back.
    //
    // Counter-factual: the yield-check exit
    // (`emit_yield_check_after_call`) returned straight from the suspend
    // helper. Every corpus test that suspends through a compiled frame passed
    // its own assertions, and only the balance check in
    // `execute_bytecode_saving_stack` — a debug build, and only once the
    // enclosing activation returned — said anything at all.
    let compiler = JitCompiler::new().expect("Failed to create compiler");
    let pop_id = compiler.helpers.pop_region_map.as_u32();
    let clif = compiler
        .clif_text(&make_suspending_call_lir(), None)
        .expect("Failed to translate");
    let refs = func_refs(&clif);

    let returns: Vec<usize> = clif
        .iter()
        .enumerate()
        .filter(|(_, line)| line.trim().starts_with("return"))
        .map(|(i, _)| i)
        .collect();
    // One per exit: the error check's, the yield check's, and the terminator's.
    assert_eq!(
        returns.len(),
        3,
        "a suspending function's Call has three exits; got:\n{}",
        clif.join("\n")
    );

    for i in returns {
        let target = call_target_before(&clif, i).unwrap_or_else(|| {
            panic!(
                "`{}` is not preceded by any call in its block:\n{}",
                clif[i].trim(),
                clif.join("\n")
            )
        });
        assert_eq!(
            refs.get(&target).copied(),
            Some(pop_id),
            "`{}` is preceded by `{target}`, not by the region-map pop \
             (u0:{pop_id}) — this exit leaks its activation's remap frame:\n{}",
            clif[i].trim(),
            clif.join("\n")
        );
    }
}
