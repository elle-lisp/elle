// audited: 2026-10-06
//! Where two frozen functions first disagree, read through their views alone.
//!
//! docs/impl/lir.md
//!
//! The views may sit over different homes — a blueprint's `Vec`s, a code
//! payload's region pages, an image's mapped pages — so the comparison reads
//! only what a reader reads, never the records behind it.

use super::view::LirView;

impl<'a> LirView<'a> {
    /// The first field, block, instruction or site at which `self` and
    /// `other` answer differently, named for a test's failure message. `None`
    /// when every answer agrees.
    ///
    /// Values compare as `Value`s do, so a closure or a string copied into
    /// another region still matches its source.
    pub fn first_difference(&self, other: &LirView<'_>) -> Option<String> {
        macro_rules! field {
            ($name:ident) => {
                if self.$name() != other.$name() {
                    return Some(format!(
                        "{}: {:?} against {:?}",
                        stringify!($name),
                        self.$name(),
                        other.$name()
                    ));
                }
            };
        }
        field!(closure_id);
        field!(name);
        field!(doc);
        field!(origin);
        field!(arity);
        field!(entry);
        field!(num_regs);
        field!(num_locals);
        field!(num_captures);
        field!(num_params);
        field!(num_local_params);
        field!(capture_params_mask);
        field!(signal);
        field!(vararg_kind);
        field!(rest_list_layout);
        field!(region_table);
        field!(merged_slots);
        field!(frame_release_slots);
        field!(frame_release_regions);
        field!(values);
        if self.capture_locals_mask().words() != other.capture_locals_mask().words() {
            return Some("capture_locals_mask".to_string());
        }
        if !self.yield_points().eq(other.yield_points()) {
            return Some("yield_points".to_string());
        }
        if !self.call_sites().eq(other.call_sites()) {
            return Some("call_sites".to_string());
        }
        if self.block_count() != other.block_count() {
            return Some(format!(
                "block_count: {} against {}",
                self.block_count(),
                other.block_count()
            ));
        }
        for (i, (a, b)) in self.blocks().zip(other.blocks()).enumerate() {
            if a.label() != b.label() || a.len() != b.len() {
                return Some(format!(
                    "block {i}: {:?} of {} nodes against {:?} of {}",
                    a.label(),
                    a.len(),
                    b.label(),
                    b.len()
                ));
            }
            for (j, (m, n)) in a.nodes().zip(b.nodes()).enumerate() {
                let same = m.op() == n.op()
                    && m.instr() == n.instr()
                    && m.span() == n.span()
                    && m.uses() == n.uses()
                    && m.def() == n.def()
                    && m.region() == n.region();
                if !same {
                    return Some(format!(
                        "block {i} node {j}: {:?} at {:?} against {:?} at {:?}",
                        m.instr(),
                        m.span(),
                        n.instr(),
                        n.span()
                    ));
                }
            }
            if a.terminator() != b.terminator() || a.terminator_span() != b.terminator_span() {
                return Some(format!(
                    "block {i} terminator: {:?} against {:?}",
                    a.terminator(),
                    b.terminator()
                ));
            }
        }
        None
    }
}
