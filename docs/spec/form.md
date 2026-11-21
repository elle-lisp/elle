# The data form

<!-- audited: 2026-11-21 -->

How Elle code reads the IR as data: one struct per function of a unit, one per block, and one per instruction.

[The region IR](ir.md) says what the IR holds, and [the judgment](judgment.md)
names the verifier and pair, which read it in this form. The rules of
[datalog](datalog.md) read facts, and an extractor written in Elle derives
those facts from this form. `compile/lir` answers this implementation's
[LIR](../impl/lir.md) in it.

## A unit

A unit is an array of function structs: the entry function first, then each
closure in the order of its closure id.

| Key | Value |
|-----|-------|
| `:id` | the closure id, or nil for the entry function |
| `:name` | the name the binder gave the lambda, or nil |
| `:arity` | `[min max]`, where `max` is nil for a function that takes any number past `min` |
| `:params` | the number of parameters |
| `:captures` | the name of each captured binding, in environment order |
| `:slots` | one entry per local slot: the name of the binding the slot holds, or nil |
| `:entry` | the label of the entry block |
| `:blocks` | the blocks, in the order the lowerer appended them |

Each parameter holds the local slot of its index. The environment holds the
captures, then one position per local slot, so an environment index below the
capture count names a capture, and any other index `i` names the slot
`i` less that count. A slot that no binding holds, such as a temporary or a
block's result, has a nil name.

## A block

| Key | Value |
|-----|-------|
| `:label` | the block's label |
| `:instrs` | the block's instructions, in order |
| `:term` | the block's terminator |

## An instruction

An instruction is a struct. Its `:kind` is the name of its variant in kebab
case, so `LoadLocal` is `:load-local`. Each field of the variant is a key, its
name in kebab case, so `closure_id` is `:closure-id` and the operator of a
`BinOp` is `:op`. `:at` holds the source position as `[line col]`, or nil where
the instruction has none.

| A field holds | Its value |
|---------------|-----------|
| a register, a label, a slot, an index, a static region, a closure id | an integer |
| a list of registers or of slots | an array of integers |
| an absent static region | nil |
| an immediate constant | the constant: nil, the empty list, a boolean, a number, a symbol or a keyword |
| a compile-time value | the value, a native function included |
| a heap literal's template | the literal, as a value |
| an offer | `[position slot]` |
| an operator or an operand proof | a keyword, as `:add`, `:int-to-float` or `:unproven` |
| signal bits | a set of keywords, one for each bit's signal |
| a symbol | the symbol |
| a flag | a boolean |

A terminator is a struct of the same shape. `Return` and `Jump` carry one
field each, named `:value` and `:target`. The fields of `Branch` are `:cond`,
`:then-label` and `:else-label`, and those of `Emit` are `:signal`, `:value`
and `:resume-label`.

## Natives

A call names its callee by a register, and a `:value-const` may load a native
function into it. A native function carries no name of its own, so
`compile/primitives` gives each native's function as `:fn` beside its name. It
also gives the region declaration this implementation reads for the native:
`:effect`, `:effect-args`, `:embeds` and `:moves-out`.
