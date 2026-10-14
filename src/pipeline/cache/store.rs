// audited: 2026-10-14
//! The store: the unit each loaded file compiled to, kept while its compile inputs match.
//!
//! docs/pipeline.md
//! docs/modules.md

use super::super::compile::{compile_file_reading, IncludeName, IncludeRead};
use super::CompileCtx;
use crate::namehash::Fnv1a;
use crate::symbol::SymbolTable;
use crate::value::fiber::SignalBits;
use crate::value::CodeUnit;
use std::collections::HashMap;

/// A digest of the compile inputs that can change within an instance: the
/// file's path, its source text, the capabilities the compile runs without,
/// and each include's resolved path and text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Digest(u64);

impl Digest {
    fn of(path: &str, source: &str, withheld: SignalBits, includes: &[IncludeRead]) -> Digest {
        let mut hash = Fnv1a::new();
        hash.write_str(path);
        hash.write_str(source);
        hash.write(&withheld.raw().to_le_bytes());
        hash.write_len(includes.len());
        for include in includes {
            hash.write_str(&include.path);
            hash.write_str(&include.text);
        }
        Digest(hash.finish())
    }
}

/// What one file's compile left: the digest of its inputs, how it named each
/// include, and its unit.
struct Kept {
    digest: Digest,
    includes: Vec<IncludeName>,
    unit: CodeUnit,
}

/// Each file a loader compiled, by its absolute path.
#[derive(Default)]
pub(super) struct Store {
    by_path: HashMap<String, Kept>,
}

impl Store {
    /// Forget every file, and give back the reference each unit holds on its
    /// code region.
    pub(super) fn clear(&mut self) {
        self.by_path.clear();
    }
}

/// `compile_file` for a loader: the unit the store keeps for `path` while
/// every compile input matches, and otherwise a fresh compile of `source`,
/// which the store then keeps in place of the old one. A compile that fails
/// keeps nothing.
pub fn compile_file_stored(
    source: &str,
    symbols: &mut SymbolTable,
    cctx: &mut CompileCtx,
    path: &str,
) -> Result<CodeUnit, String> {
    if let Some(unit) = cctx.kept(symbols, source, path) {
        return Ok(unit);
    }
    let (unit, includes) = compile_file_reading(source, symbols, cctx, path)?;
    let kept = Kept {
        digest: Digest::of(path, source, cctx.withheld, &includes),
        includes: includes.into_iter().map(|read| read.name).collect(),
        unit: unit.clone(),
    };
    cctx.store.by_path.insert(path.to_string(), kept);
    Ok(unit)
}

impl CompileCtx {
    /// The unit kept for `path`, when `source`, the capabilities this compile
    /// runs without and every include read now digest as they did for the
    /// compile that left it.
    fn kept(&mut self, symbols: &mut SymbolTable, source: &str, path: &str) -> Option<CodeUnit> {
        let names = self.store.by_path.get(path)?.includes.clone();
        let includes = self.read_again(symbols, &names)?;
        let kept = self.store.by_path.get(path)?;
        let digest = Digest::of(path, source, self.withheld, &includes);
        (kept.digest == digest).then(|| kept.unit.clone())
    }

    /// Resolve each of `names` again and read the file it names now, as a
    /// compile does: on the macro VM, without the capabilities the compile
    /// runs without. None when one names no file or cannot be read, or when
    /// the compile may read no file, so that the compile reports why.
    fn read_again(
        &mut self,
        symbols: &mut SymbolTable,
        names: &[IncludeName],
    ) -> Option<Vec<IncludeRead>> {
        if names.is_empty() {
            return Some(Vec::new());
        }
        if self.withheld.intersects(crate::signals::SIG_FS) {
            return None;
        }
        self.vm.reset_fiber();
        self.vm.fiber.withheld = self.withheld;
        self.vm.set_symbols(symbols as *mut SymbolTable);
        names
            .iter()
            .map(|name| {
                let path = name.resolve(&mut self.vm, &self.meta).ok()?;
                let text = std::fs::read_to_string(&path).ok()?;
                Some(IncludeRead::new(name.clone(), path, text))
            })
            .collect()
    }
}
