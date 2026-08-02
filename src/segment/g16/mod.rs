//! Vendored Unicode 16.0.0 grapheme segmentation — generation G16.
//!
//! Frozen copy of the `unicode-segmentation` crate, version 1.12.0,
//! reduced to the grapheme engine. `tables.rs` is byte-identical to the
//! upstream release; `grapheme.rs` differs only in `use crate::tables`
//! becoming `use super::tables`, plus one upstream bugfix from 1.13.x:
//! the reverse InCB scan stores its countdown in `incb_linker_count`
//! (1.12.0 wrote it to `ris_count`, breaking reverse iteration over
//! conjuncts). Upstream is dual-licensed MIT/Apache-2.0; the license
//! texts and COPYRIGHT notice ship in this directory.
//!
//! Old table generations never change. Do not edit these files; a table
//! fix belongs in a NEW generation.

#[allow(clippy::all, dead_code)]
#[rustfmt::skip]
pub(crate) mod grapheme;
#[allow(clippy::all, dead_code, non_upper_case_globals)]
#[rustfmt::skip]
pub(crate) mod tables;
