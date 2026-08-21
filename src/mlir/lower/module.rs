use super::*;

mod ctx;
mod instr;
mod term;

use ctx::LowerCtx;

/// Lower a GPU-eligible LirFunction into an MLIR module.
///
/// The module contains a single `func.func` with `llvm.emit_c_interface`
/// so the execution engine can call it via C calling convention.
///
/// Local slots are allocated with `memref.alloca` in the entry block
/// for correct cross-block semantics (phi-like patterns via memory).
///
/// Setup (signature, block/argument construction, local allocas) happens here;
/// per-instruction and per-terminator emission is delegated to
/// [`instr::lower_instr`] and [`term::lower_terminator`], which append ops to
/// the same blocks in the same order as a single flat pass would.
pub fn lower_to_module<'c>(
    context: &'c Context,
    lir: &LirFunction,
    num_captures: u16,
    capture_types: u64,
    param_types: u64,
) -> Result<(Module<'c>, ScalarType), String> {
    // Pre-scan for cross-block mixed-type slots before building any MLIR ops.
    check_slot_types(lir, num_captures, capture_types, param_types)?;

    let location = Location::unknown(context);
    let module = Module::new(location);

    let i64_type: Type = IntegerType::new(context, 64).into();
    let f64_type: Type = Type::float64(context);
    let num_params = lir.arity.fixed_params();
    let total_args = num_captures as usize + num_params;

    let mlir_param_types: Vec<Type> = (0..total_args).map(|_| i64_type).collect();
    let func_type = FunctionType::new(context, &mlir_param_types, &[i64_type]);
    let func_name = lir.name.as_deref().unwrap_or("gpu_kernel");

    let region = Region::new();

    // Map LIR labels to block indices
    let mut label_to_idx: HashMap<Label, usize> = HashMap::new();
    let mut blocks: Vec<Block> = Vec::new();

    for (i, lir_block) in lir.blocks.iter().enumerate() {
        let block = if i == 0 {
            Block::new(
                &mlir_param_types
                    .iter()
                    .map(|t| (*t, location))
                    .collect::<Vec<_>>(),
            )
        } else {
            Block::new(&[])
        };
        label_to_idx.insert(lir_block.label, i);
        blocks.push(block);
    }

    // Shared lowering state. `env_vals` is kept separate from `regs` so
    // LoadCapture lookups are never clobbered by destination register writes
    // (LIR reg indices can collide with env indices).
    let mut ctx = LowerCtx {
        context,
        location,
        i64_type,
        f64_type,
        total_args,
        regs: HashMap::new(),
        types: HashMap::new(),
        env_vals: HashMap::new(),
        slot_types: HashMap::new(),
        local_slots: HashMap::new(),
        return_type: None,
    };

    // Allocate memref slots for locals in the entry block.
    // Local slots handle cross-block value passing (phi patterns).
    let scalar_memref = MemRefType::new(i64_type, &[], None, None);
    let num_locals = lir.num_locals as u32;

    if !blocks.is_empty() {
        let entry = &blocks[0];

        // Pre-populate env_vals with entry block arguments.
        // MLIR signature: [captures..., params...], all i64.
        // Captures marked as Float in capture_types get bitcast i64→f64.
        for i in 0..num_captures as usize {
            let raw: Value = entry.argument(i).unwrap().into();
            if capture_types & (1u64 << i) != 0 {
                let bc = entry.append_operation(arith::bitcast(raw, f64_type, location));
                ctx.env_vals.insert(
                    EnvIndex::new(i as u32),
                    (bc.result(0).unwrap().into(), ScalarType::Float),
                );
            } else {
                ctx.env_vals
                    .insert(EnvIndex::new(i as u32), (raw, ScalarType::Int));
            }
        }
        // Params follow captures in the MLIR argument list.
        for i in 0..num_params {
            let arg_idx = num_captures as usize + i;
            let raw: Value = entry.argument(arg_idx).unwrap().into();
            if param_types & (1u64 << i) != 0 {
                let bc = entry.append_operation(arith::bitcast(raw, f64_type, location));
                ctx.env_vals.insert(
                    EnvIndex::new(arg_idx as u32),
                    (bc.result(0).unwrap().into(), ScalarType::Float),
                );
            } else {
                ctx.env_vals
                    .insert(EnvIndex::new(arg_idx as u32), (raw, ScalarType::Int));
            }
        }

        // Allocate a memref<i64> for each local slot
        for slot in 0..num_locals {
            let alloca_op = entry.append_operation(memref::alloca(
                context,
                scalar_memref,
                &[],
                &[],
                None,
                location,
            ));
            ctx.local_slots
                .insert(SlotId::new(slot), alloca_op.result(0).unwrap().into());
        }
    }

    // Lower instructions and terminators, block by block. `entry` (block 0)
    // is passed for the LoadCapture fallback path.
    let entry_block = &blocks[0];
    for (block_idx, lir_block) in lir.blocks.iter().enumerate() {
        let block = &blocks[block_idx];

        for si in &lir_block.instructions {
            instr::lower_instr(&mut ctx, block, entry_block, si)?;
        }

        term::lower_terminator(&mut ctx, block, &blocks, &label_to_idx, lir_block)?;
    }

    let return_type = ctx.return_type;
    for block in blocks {
        region.append_block(block);
    }

    let func_op = func::func(
        context,
        StringAttribute::new(context, func_name),
        TypeAttribute::new(func_type.into()),
        region,
        &[(
            melior::ir::Identifier::new(context, "llvm.emit_c_interface"),
            melior::ir::attribute::Attribute::unit(context),
        )],
        location,
    );
    module.body().append_operation(func_op);

    if !module.as_operation().verify() {
        return Err("MLIR verification failed".to_string());
    }

    Ok((module, return_type.unwrap_or(ScalarType::Int)))
}
