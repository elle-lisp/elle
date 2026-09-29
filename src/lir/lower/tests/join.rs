// audited: 2026-09-29
//! That the lowerer puts a `JoinRegion` right before an admitted push site's allocation, and nowhere else.
//!
//! docs/impl/region/colocation.md
//!
//! The instruction is read through its printed form, `join-region <slot>
//! partner=<reg>`. The call after it must name the same slot: the runtime hands a
//! pending join to the next value mint, and only the slot that follows consumes
//! it.

use super::*;

/// Each `join-region` in `module`, with the instruction right after it.
fn joins_in(module: &crate::lir::LirModule) -> Vec<(String, Option<LirInstr>)> {
    let mut out = Vec::new();
    for func in std::iter::once(&module.entry).chain(module.closures.iter()) {
        for block in &func.blocks {
            let instrs = &block.instructions;
            for (i, si) in instrs.iter().enumerate() {
                let text = si.instr.to_string();
                if text.starts_with("join-region ") {
                    out.push((text, instrs.get(i + 1).map(|n| n.instr.clone())));
                }
            }
        }
    }
    out
}

#[test]
fn an_admitted_push_joins_right_before_the_pushed_values_allocation() {
    let module = compile_to_lir(
        "(let [build (fn [n]
                       (let [out @[]]
                         (%push-array-mut out [n n])
                         out))]
           (build 1))",
    );
    let joins = joins_in(&module);
    assert_eq!(joins.len(), 1, "one push site, one join: {joins:?}");
    let (text, next) = &joins[0];
    let Some(LirInstr::Call { region, args, .. }) = next else {
        panic!("the join precedes the pushed value's call, got {next:?}");
    };
    assert_eq!(args.len(), 2, "the call after the join builds `[n n]`");
    assert!(
        text.starts_with(&format!("join-region {region} partner=")),
        "the join names the slot of the call after it: {text}",
    );
}

#[test]
fn a_container_that_loses_a_value_gets_no_join() {
    let module = compile_to_lir(
        "(let [build (fn [n]
                       (let [out @[]]
                         (%push-array-mut out [n n])
                         (%pop out)
                         out))]
           (build 1))",
    );
    assert!(
        joins_in(&module).is_empty(),
        "a pop refuses the whole container"
    );
}
