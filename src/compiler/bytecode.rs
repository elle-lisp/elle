use crate::error::LocationMap;
use crate::reader::SourceLoc;
use crate::value::Value;

mod disasm;
pub use disasm::*;

mod instruction;
pub use instruction::*;

/// Compiled bytecode with constants
#[derive(Debug, Clone)]
pub struct Bytecode {
    pub instructions: Vec<u8>,
    pub constants: Vec<Value>,
    /// Bytecode offset → source location mapping for error reporting.
    /// Maps instruction offsets to their source locations.
    pub location_map: LocationMap,
    /// Inferred signal of the top-level expression. Carried through the
    /// pipeline so that `execute_scheduled` can build a thunk with the
    /// correct signal metadata for fiber scheduling and shared allocator
    /// provisioning.
    pub signal: crate::signals::Signal,
    /// Signal projection: maps keyword field names to the signals of exported
    /// closures. Populated by `compute_signal_projection` during file-scope
    /// compilation. When an importing file sees `module:field`, the analyzer
    /// uses this projection instead of the conservative `Polymorphic` fallback.
    pub signal_projection: Option<std::collections::HashMap<String, crate::signals::Signal>>,
    /// Blueprints for this code object's `MakeClosure` instructions. Each
    /// `MakeClosure` pushes its nested-lambda blueprint here and emits the index;
    /// the VM/JIT materialize a fresh region-allocated `HeapObject::ClosureTemplate`
    /// header per execution (a heap literal is an ordinary, reclaimable
    /// allocation). Threaded into the executing `Code` so each header is
    /// reclaimed by region RC, never pinned for the process lifetime.
    pub child_protos: Vec<std::rc::Rc<crate::value::TemplateProto>>,
    /// The static region slots this (top-level / entry) function's allocations
    /// SHARE after a builder-idiom merge (docs/impl/region/merging.md § Merging),
    /// carried from the entry `LirFunction.merged_slots` so the executing `Code`
    /// can mint-or-reuse them. The per-lambda equivalent rides
    /// the per-lambda blueprint's `merged_slots`; this is the entry-function path
    /// (`Bytecode → Code`), which would otherwise read empty. Empty unless a merge
    /// fired (a builder idiom seeded by a nested `%pair` literal), so inert when
    /// no merge exists.
    pub merged_slots: rustc_hash::FxHashSet<u32>,
    /// The local slots this (top-level / entry) function's value-routed releases
    /// read, carried from the entry `LirFunction.frame_release_slots` so the
    /// executing `Code` can walk them at an error exit
    /// (docs/impl/region/mechanism.md § "An abandoned frame runs the releases it
    /// still owes"). The per-lambda equivalent rides
    /// the per-lambda blueprint's `frame_release_slots`; this is the
    /// entry-function path (`Bytecode → Code`), which would otherwise read empty.
    pub frame_release_slots: Vec<u16>,
    /// The `DecrefRegion` half of the same table, carried the same way — the
    /// static region slots this entry function's slot-routed releases name.
    pub frame_release_regions: Vec<u32>,
}

impl Bytecode {
    pub fn new() -> Self {
        Bytecode {
            instructions: Vec::new(),
            constants: Vec::new(),
            location_map: LocationMap::new(),
            signal: crate::signals::Signal::silent(),
            signal_projection: None,
            child_protos: Vec::new(),
            merged_slots: rustc_hash::FxHashSet::default(),
            frame_release_slots: Vec::new(),
            frame_release_regions: Vec::new(),
        }
    }

    /// This compiled unit as a code-object blueprint: the entry function's own
    /// bytecode, constants, locations and region tables, plus the
    /// nested-lambda blueprints its `MakeClosure` instructions index.
    ///
    /// The entry paths materialize this exactly as `MakeClosure` materializes a
    /// nested blueprint, so a top-level or module body reaches its bytecode the
    /// way every other code object does. Arity is nullary: a compiled unit is
    /// entered with no arguments.
    pub fn into_proto(self) -> crate::value::TemplateProto {
        crate::value::TemplateProto {
            signal: self.signal,
            location_map: self.location_map,
            child_protos: self.child_protos,
            merged_slots: self.merged_slots,
            frame_release_slots: self.frame_release_slots,
            frame_release_regions: self.frame_release_regions,
            ..crate::value::TemplateProto::new(
                self.instructions,
                crate::value::Arity::Exact(0),
                self.constants,
            )
        }
    }

    /// Record a source location for the current bytecode position.
    /// Only records non-synthetic spans (line > 0).
    pub fn record_location(&mut self, span: &crate::syntax::Span) {
        // Skip synthetic spans (all zeros)
        if span.line == 0 && span.col == 0 && span.start == 0 && span.end == 0 {
            return;
        }

        let offset = self.current_pos();
        let loc = SourceLoc::new(
            span.file().unwrap_or("<input>"),
            span.line as usize,
            span.col as usize,
        );
        self.location_map.insert(offset, loc);
    }

    /// Add a constant and return its index
    pub fn add_constant(&mut self, value: Value) -> u16 {
        // Check if constant already exists
        for (i, c) in self.constants.iter().enumerate() {
            if c == &value {
                return i as u16;
            }
        }

        let idx = self.constants.len();
        if idx > u16::MAX as usize {
            panic!("Too many constants");
        }
        self.constants.push(value);
        idx as u16
    }

    /// Emit an instruction
    pub fn emit(&mut self, instr: Instruction) {
        self.instructions.push(instr as u8);
    }

    /// Emit a byte
    pub fn emit_byte(&mut self, byte: u8) {
        self.instructions.push(byte);
    }

    /// Emit a u16 (big-endian)
    pub fn emit_u16(&mut self, value: u16) {
        self.instructions.push((value >> 8) as u8);
        self.instructions.push((value & 0xff) as u8);
    }

    /// Emit a u32 (big-endian). Used for region-id operands, which are
    /// minted fresh per allocation-site and thus need the full 32-bit
    /// space (see docs/regions/semantics.md — every value its own region).
    pub fn emit_u32(&mut self, value: u32) {
        self.instructions.push((value >> 24) as u8);
        self.instructions.push((value >> 16) as u8);
        self.instructions.push((value >> 8) as u8);
        self.instructions.push((value & 0xff) as u8);
    }

    /// Emit a `SignalBits` operand: eight bytes, big-endian.
    ///
    /// Every bit of the mask is meaningful — user signals live at bits 32-63 —
    /// so this is the only way to write one. See `docs/impl/bytecode.md`
    /// § "Signal-bits operands"; [`crate::vm::VM::read_signal_bits`] reads it.
    pub fn emit_signal_bits(&mut self, bits: crate::value::fiber::SignalBits) {
        self.instructions
            .extend_from_slice(&bits.raw().to_be_bytes());
    }

    /// Emit an i16 (big-endian)
    pub fn emit_i16(&mut self, value: i16) {
        self.emit_u16(value as u16);
    }

    /// Emit an i32 (big-endian)
    pub fn emit_i32(&mut self, value: i32) {
        let bytes = value.to_be_bytes();
        self.instructions.push(bytes[0]);
        self.instructions.push(bytes[1]);
        self.instructions.push(bytes[2]);
        self.instructions.push(bytes[3]);
    }

    /// Get current position for jump patching
    pub fn current_pos(&self) -> usize {
        self.instructions.len()
    }

    /// Patch a jump instruction at a given position (i32 big-endian)
    pub fn patch_jump(&mut self, pos: usize, offset: i32) {
        let bytes = offset.to_be_bytes();
        self.instructions[pos] = bytes[0];
        self.instructions[pos + 1] = bytes[1];
        self.instructions[pos + 2] = bytes[2];
        self.instructions[pos + 3] = bytes[3];
    }

    pub fn patch_u16(&mut self, pos: usize, value: u16) {
        self.instructions[pos] = (value >> 8) as u8;
        self.instructions[pos + 1] = (value & 0xff) as u8;
    }
}

impl Default for Bytecode {
    fn default() -> Self {
        Self::new()
    }
}

// ── Debug formatting ────────────────────────────────────────────────

/// Disassemble bytecode with proper instruction names and operands
pub fn disassemble(instructions: &[u8]) -> String {
    disassemble_lines(instructions)
        .iter()
        .map(|line| format!("  {}", line))
        .collect::<Vec<_>>()
        .join("\n")
        + "\n"
}

/// Pretty print bytecode with constants
pub fn format_bytecode_with_constants(instructions: &[u8], constants: &[crate::Value]) -> String {
    let mut output = String::new();
    output.push_str("Bytecode:\n");
    output.push_str(&disassemble(instructions));
    output.push_str("\nConstants:\n");
    for (i, c) in constants.iter().enumerate() {
        output.push_str(&format!("  [{}] = {:?}\n", i, c));
    }
    output
}

/// Pretty print a whole compiled unit: the entry bytecode plus every nested
/// lambda's template (`child_protos`), recursively, each labeled by its
/// `MakeClosure` const_idx path so a dump can be matched to the instruction
/// that materializes it.
pub fn format_bytecode_with_protos(bytecode: &Bytecode) -> String {
    let mut output = format_bytecode_with_constants(&bytecode.instructions, &bytecode.constants);
    for (i, proto) in bytecode.child_protos.iter().enumerate() {
        format_proto(&mut output, &format!("{}", i), proto);
    }
    output
}

fn format_proto(output: &mut String, path: &str, proto: &crate::value::TemplateProto) {
    output.push_str(&format!(
        "\n── proto [{}] {} (captures={}, params={}) ──\n",
        path,
        proto.name.as_deref().unwrap_or("<anon>"),
        proto.num_captures,
        proto.num_params,
    ));
    output.push_str(&disassemble(&proto.bytecode));
    for (i, child) in proto.child_protos.iter().enumerate() {
        format_proto(output, &format!("{path}.{i}"), child);
    }
}

#[cfg(test)]
mod tests;
