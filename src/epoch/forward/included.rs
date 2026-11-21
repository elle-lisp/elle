// audited: 2026-11-20
//! The reads a file an include splices makes of a name its body defines after the include, which the epoch 18 rule leaves.
//!
//! docs/epochs/bodies.md
//! docs/include.md
//!
//! The rule reads no included file, so only `elle rewrite` asks for these
//! reads, and it reads each file for them. Up to epoch 17 the forms an include
//! splices join the body that holds it, so a def in the included file binds
//! its name for the whole body, and a read inside a lambda there finds a def
//! after the include.

use super::reading::Reading;
use super::scan::form_reads;
use crate::pipeline::IncludeName;
use crate::syntax::{Span, Syntax};

/// A read in an included file of a name the body that holds the include
/// defines after it.
pub(crate) struct IncludedRead {
    /// The include in the file the rule reads, which the note points at.
    pub include: Span,
    pub name: String,
    /// Where the read stands in the included file.
    pub at: Span,
}

/// Reads the file an include names: its absolute path and its forms, or None
/// when it cannot.
pub(crate) type ReadInclude<'r> = dyn FnMut(&IncludeName) -> Option<(String, Vec<Syntax>)> + 'r;

/// Each read inside a lambda that a file an include in `forms` splices makes
/// of a name the include's body defines after it. `read` reads each file. The
/// reads in a file that an included file includes count as the reads of that
/// file, but a file already being read is a cycle, and is read no further.
pub(crate) fn included_reads(forms: &[Syntax], read: &mut ReadInclude<'_>) -> Vec<IncludedRead> {
    let mut out = Vec::new();
    super::bodies::each(forms, &mut |_, items, start| {
        let Some(reading) = Reading::of(items, start) else {
            return;
        };
        for (k, slot) in reading.slots.iter().enumerate() {
            let Some(name) = IncludeName::of(slot.node) else {
                continue;
            };
            let mut later: Vec<&str> = Vec::new();
            for form in &reading.read[k + 1..] {
                for name in &form.names {
                    if !later.contains(name)
                        && !reading.read[..k].iter().any(|f| f.names.contains(name))
                    {
                        later.push(name);
                    }
                }
            }
            if later.is_empty() {
                continue;
            }
            let mut file = File {
                include: slot.node.span,
                chain: Vec::new(),
                read: &mut *read,
                out: &mut out,
            };
            file.enter(&name, &later);
        }
    });
    out
}

/// One include in the file the rule reads, and the files it reads through.
struct File<'f, 'r> {
    include: Span,
    /// The path of each file being read, outermost first.
    chain: Vec<String>,
    read: &'f mut ReadInclude<'r>,
    out: &'f mut Vec<IncludedRead>,
}

impl File<'_, '_> {
    /// Read the file `name` names, and push its reads of `names`, the names a
    /// def after the include binds.
    fn enter(&mut self, name: &IncludeName, names: &[&str]) {
        let Some((path, forms)) = (self.read)(name) else {
            return;
        };
        if self.chain.contains(&path) {
            return;
        }
        let Some(reading) = Reading::of(&forms, 0) else {
            return;
        };
        // A def in the file binds its name for the whole body, so a read of
        // that name finds it.
        let own: Vec<&str> = reading
            .read
            .iter()
            .flat_map(|f| f.names.iter().copied())
            .collect();
        let names: Vec<&str> = names.iter().copied().filter(|n| !own.contains(n)).collect();
        if names.is_empty() {
            return;
        }
        self.chain.push(path);
        for slot in &reading.slots {
            match IncludeName::of(slot.node) {
                Some(inner) => self.enter(&inner, &names),
                None => {
                    for read in form_reads(slot.node, &names) {
                        if read.in_lambda {
                            self.out.push(IncludedRead {
                                include: self.include,
                                name: read.name.to_string(),
                                at: read.at,
                            });
                        }
                    }
                }
            }
        }
        self.chain.pop();
    }
}
