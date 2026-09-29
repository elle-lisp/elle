// audited: 2026-09-29
// docs/impl/jit.md
//! Where a store into a stack slot puts a `Value`: the tag and the payload at
//! the offsets a load reads, at the stride the runtime walks.

use super::*;
use crate::jit::translate::{finalize_function, store_value_slot};
use crate::value::Value;
use cranelift_codegen::ir::{MemFlagsData, StackSlotData, StackSlotKind};

const WORD: usize = std::mem::size_of::<u64>();
const STRIDE: usize = std::mem::size_of::<Value>();
const TAG: usize = std::mem::offset_of!(Value, tag);
const PAYLOAD: usize = std::mem::offset_of!(Value, payload);

/// Marks a word no store wrote. Neither half of a probe `Value` equals it.
const UNWRITTEN: u64 = 0xAAAA_AAAA_AAAA_AAAA;

/// One `store_value_slot` call: the `Value`'s index, its tag half, its payload half.
type Write = (u32, u64, u64);

/// Compile `fn(out)`: fill a stack slot of `values` `Value`s with `UNWRITTEN`,
/// apply each of `writes` through `store_value_slot`, then copy the slot's
/// words to `out` with plain loads at fixed offsets. Run it and return the words.
///
/// The copy reads raw words rather than going back through `load_value_slot`,
/// so a store and a load that disagree with the layout in the same way cannot
/// agree with each other.
fn slot_words_after(values: usize, writes: &[Write]) -> Vec<u64> {
    let mut compiler = JitCompiler::new().expect("Failed to create compiler");
    let mut sig = compiler.module.make_signature();
    sig.call_conv = CallConv::SystemV;
    sig.params.push(AbiParam::new(I64));
    let func_id = compiler
        .module
        .declare_function("store_probe", Linkage::Local, &sig)
        .expect("declare");

    let mut ctx = compiler.module.make_context();
    ctx.func.signature = sig;
    ctx.func.name = UserFuncName::user(0, func_id.as_u32());

    let words = values * STRIDE / WORD;
    let mut builder_ctx = FunctionBuilderContext::new();
    let mut builder = FunctionBuilder::new(&mut ctx.func, &mut builder_ctx);
    let entry = builder.create_block();
    builder.append_block_params_for_function_params(entry);
    builder.switch_to_block(entry);
    builder.seal_block(entry);
    let out = builder.block_params(entry)[0];

    let slot = builder.create_sized_stack_slot(StackSlotData::new(
        StackSlotKind::ExplicitSlot,
        (values * STRIDE) as u32,
        0,
    ));
    let filler = builder.ins().iconst(I64, UNWRITTEN as i64);
    let base = builder.ins().stack_addr(I64, slot, 0);
    for word in 0..words {
        builder
            .ins()
            .store(MemFlagsData::trusted(), filler, base, (word * WORD) as i32);
    }
    for &(index, tag, payload) in writes {
        let tag = builder.ins().iconst(I64, tag as i64);
        let payload = builder.ins().iconst(I64, payload as i64);
        store_value_slot(&mut builder, slot, index, tag, payload);
    }
    let base = builder.ins().stack_addr(I64, slot, 0);
    for word in 0..words {
        let offset = (word * WORD) as i32;
        let value = builder
            .ins()
            .load(I64, MemFlagsData::trusted(), base, offset);
        builder
            .ins()
            .store(MemFlagsData::trusted(), value, out, offset);
    }
    builder.ins().return_(&[]);
    finalize_function(builder, &compiler.module);

    compiler
        .module
        .define_function(func_id, &mut ctx)
        .expect("define");
    compiler.module.finalize_definitions().expect("finalize");
    let entry = compiler.module.get_finalized_function(func_id);
    let probe = unsafe { std::mem::transmute::<*const u8, extern "C" fn(*mut u64)>(entry) };

    let mut out = vec![0u64; words];
    probe(out.as_mut_ptr());
    out
}

/// The words a slot of `values` `Value`s holds when only `writes` ran, by the
/// layout of `Value` and nothing else.
fn expected_words(values: usize, writes: &[Write]) -> Vec<u64> {
    let mut words = vec![UNWRITTEN; values * STRIDE / WORD];
    for &(index, tag, payload) in writes {
        let base = index as usize * STRIDE;
        words[(base + TAG) / WORD] = tag;
        words[(base + PAYLOAD) / WORD] = payload;
    }
    words
}

#[test]
fn a_stored_value_puts_its_tag_and_payload_at_the_value_layout() {
    // Trap: the test reads raw words at fixed offsets. A helper that stored
    // the halves swapped, or at `index * 8`, still writes two words into the
    // slot, and only their positions tell it apart.
    //
    // Counter-factual: a helper that emitted nothing leaves every word
    // `UNWRITTEN`, and the expected words differ in every written position.
    let writes: [Write; 3] = [
        (0, 0x1111_0000_0000_0001, 0x2222_0000_0000_0001),
        (1, 0x1111_0000_0000_0002, 0x2222_0000_0000_0002),
        (2, 0x1111_0000_0000_0003, 0x2222_0000_0000_0003),
    ];
    assert_eq!(slot_words_after(3, &writes), expected_words(3, &writes));
}

#[test]
fn a_store_leaves_the_neighbouring_values_alone() {
    // A stride of 8 or 24 lands index 2's words on index 1's, or past the slot.
    // Index 1 is never written, so its two words must still be `UNWRITTEN`.
    let writes: [Write; 2] = [
        (2, 0x3333_0000_0000_0002, 0x4444_0000_0000_0002),
        (0, 0x3333_0000_0000_0000, 0x4444_0000_0000_0000),
    ];
    assert_eq!(slot_words_after(3, &writes), expected_words(3, &writes));
}

#[test]
fn a_second_store_to_an_index_replaces_the_first() {
    let writes: [Write; 2] = [
        (1, 0x5555_0000_0000_0001, 0x6666_0000_0000_0001),
        (1, 0x5555_0000_0000_0002, 0x6666_0000_0000_0002),
    ];
    assert_eq!(slot_words_after(2, &writes), expected_words(2, &writes));
}
