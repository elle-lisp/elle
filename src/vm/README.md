# Virtual Machine

The VM executes bytecode produced by the compiler. It's a stack-based machine
with register-addressed local variables and a fiber-based signal system.

## Architecture

**Operand Stack**: Most operations push and pop from a central stack. Add pops
two values, adds them, pushes the result.

**Local Variables**: Inside closures, locals live in the closure's environment
vector, accessed by index via `LoadUpvalue`/`StoreUpvalue`. At top level,
locals use `LoadLocal`/`StoreLocal` which access the stack directly.

**Primitives**: Primitives and stdlib functions are injected into the
outermost scope during compilation. There are no runtime globals — all
variable references (including primitives) are resolved at analysis time
and accessed as upvalues in the compiled closure.

## Execution Loop

```rust
loop {
    let instr = bytecode[ip];
    ip += 1;

    match instr {
        LoadConst => { /* push constant */ }
        Add => { /* pop two, push sum */ }
        Call => { /* complex: setup env, recurse */ }
        Return => { return stack.pop(); }
        // ...
    }

    // Check for signals after each instruction
    if let Some((bits, _)) = fiber.signal {
        // propagate signal to parent
    }
}
```

## Closure Calls

When calling a closure:

1. Validate arity
2. Build environment: `[captures..., params..., locals...]`
3. Wrap parameters in lboxes if `lbox_params_mask` indicates
4. Push call frame for stack traces
5. Recursively execute closure's bytecode
6. Pop frame, push result

## Tail Calls

Tail calls (`TailCall` instruction) avoid stack growth:

1. Build new environment
2. Store in `pending_tail_call` instead of recursing
3. Return dummy value
4. Outer loop detects pending call, loops back

This enables unbounded recursion without stack overflow.

## Signal-Based Error Handling

Errors are signals, not exceptions. When a runtime error occurs:

1. The VM (or primitive) stores `(SIG_ERROR, error_value)` in `fiber.signal`
2. The dispatch loop checks `fiber.signal` after each instruction
3. The signal propagates up the fiber chain
4. A parent fiber with the `:error` bit in its mask catches it

`try`/`catch` is a macro that creates a child fiber with mask=`SIG_ERROR`,
resumes it, and checks the result. No special VM support beyond the fiber
primitive.

## Fibers

Fibers are independent execution contexts (stack + frames + signal mask).
See `AGENTS.md` and `docs/fibers.md` for the full fiber architecture.

## See Also

- [AGENTS.md](AGENTS.md) - technical reference for LLM agents
- `src/compiler/bytecode.rs` - instruction definitions
- `src/lir/emit.rs` - bytecode emission
- `docs/fibers.md` - fiber architecture
- `docs/signals.md` - signal system design
