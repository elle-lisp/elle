// Bytecode-boundary safety: malformed bytecode must have DEFINED behavior.
//
// `VM::execute_bytecode` (src/vm/mod.rs) is a `pub` API taking `&[u8]`, so the
// dispatch loop (src/vm/dispatch/interp.rs) must decode opcodes with a checked
// conversion: `Instruction` has ~117 variants, and the other ~139 byte values
// must NOT reach a bare `transmute`, which would be undefined behavior (in debug
// builds rustc's enum-construction check turns it into a non-unwinding panic
// that SIGABRTs the harness; in release builds there is no check at all).
//
// The defined behavior is a catchable (unwinding) "VM bug" panic, matching
// the adjacent end-of-bytecode check: bytecode is produced in-process by the
// compiler, so an invalid opcode means a compiler bug or a corrupted buffer,
// and the error path must not allocate (no heap region is guaranteed to be
// active when decoding fails).
#[test]
#[should_panic(expected = "invalid opcode")]
fn invalid_opcode_byte_panics_with_defined_message() {
    let mut vm = elle::vm::VM::new();
    // 0xFF is not a valid Instruction discriminant. A hand-fed byte buffer
    // carries no region tables — the blueprint's are empty by construction.
    let proto = std::rc::Rc::new(elle::value::TemplateProto::new(
        vec![0xFF],
        elle::value::Arity::Exact(0),
        Vec::new(),
    ));
    let _ = vm.execute_proto(&proto, None);
}
