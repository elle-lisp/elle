// audited: 2026-11-22
//! `include-file`: join each include's path, check the chain of files being included, and read the file.
//!
//! docs/include.md
//! src/pipeline/overview.md
//!
//! The expander splices what an include reads (src/syntax/expand/include.rs).
//! `splice_includes` splices the top level of source text alone, for the WASM
//! backend.

use super::*;
use crate::value::SignalBits;

/// Splice include-file directives in source text.
///
/// Reads top-level forms, resolves includes, and returns a single string
/// with all included content inlined. Used by the WASM backend to resolve
/// includes before wrapping user code in ev/run. The includes read under the
/// capabilities `cctx` withholds.
pub fn splice_includes(
    source: &str,
    source_name: &str,
    cctx: &CompileCtx,
) -> Result<String, String> {
    // Text in, text out: the trees exist only to be re-rendered, so they get
    // their own heap and die with it.
    let mut home = crate::syntax::SyntaxHeap::new();
    let arena = home.arena();
    let syntaxes = read_syntax_all_for(arena, source, source_name)?;
    let mut parts: Vec<String> = Vec::new();
    let mut includes = Includes::default();
    splice_text(&arena, syntaxes, cctx.withheld(), &mut includes, &mut parts)?;
    Ok(parts.join("\n"))
}

/// Render each of `forms` onto `parts`, and in place of an include, the forms
/// of the file it names.
fn splice_text(
    arena: &SyntaxArena,
    forms: Vec<Syntax>,
    withheld: SignalBits,
    includes: &mut Includes,
    parts: &mut Vec<String>,
) -> Result<(), String> {
    for form in forms {
        let Some(name) = IncludeName::of(&form) else {
            parts.push(format!("{}", form));
            continue;
        };
        let (read, mark) = includes.enter(arena, &form, name, withheld)?;
        let spliced = splice_text(arena, read, withheld, includes, parts);
        includes.leave(mark);
        spliced?;
    }
    Ok(())
}

/// How an include names its file: the path it wrote, and the including file's
/// directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct IncludeName {
    path: String,
    dir: Option<String>,
}

impl IncludeName {
    /// The name `(include-file "path")` writes, or None when `form` is not
    /// one. The directory is that of the file the form was read from, so an
    /// include inside an included file resolves against that file.
    pub(crate) fn of(form: &Syntax) -> Option<IncludeName> {
        let [head, path] = form.as_list()? else {
            return None;
        };
        if head.as_symbol()? != "include-file" {
            return None;
        }
        let SyntaxKind::String(path) = &path.kind else {
            return None;
        };
        let dir = form
            .span
            .source_path()
            .and_then(|f| crate::path::parent(&f).map(str::to_string));
        Some(IncludeName {
            path: path.to_string(),
            dir,
        })
    }

    /// The path joined to the including file's directory, or to the working
    /// directory for code with no file, or None when it names no file. The
    /// answer is absolute and normalized, the spelling the circular-include
    /// check keys on.
    pub(crate) fn resolve(&self) -> Option<String> {
        let joined = match &self.dir {
            Some(dir) => crate::path::join(&[dir, &self.path]),
            None => self.path.clone(),
        };
        let path = crate::path::absolute(&joined).ok()?;
        crate::path::is_file(&path).then_some(path)
    }
}

/// The length of the chain before an include entered it, which
/// [`Includes::leave`] restores.
#[must_use]
pub(crate) struct ChainMark(usize);

/// The chain of files being included, which the circular-include check reads.
#[derive(Default)]
pub(crate) struct Includes {
    /// The file that holds the outermost include, then each file an include
    /// entered, outermost first.
    chain: Vec<String>,
}

impl Includes {
    /// Resolve the include `form`, which names `name`, and read its file's
    /// forms. The file enters the chain until [`leave`](Self::leave) takes the
    /// mark back.
    ///
    /// An include reads a file for the compile, so it reads under `withheld`,
    /// the capabilities of the fiber the compile runs for, as a macro does. A
    /// file already in the chain is a cycle, and so is the file holding `form`.
    pub(crate) fn enter(
        &mut self,
        arena: &SyntaxArena,
        form: &Syntax,
        name: IncludeName,
        withheld: SignalBits,
    ) -> Result<(Vec<Syntax>, ChainMark), String> {
        let span = form.span;
        if withheld.intersects(crate::signals::SIG_FS) {
            return Err(format!(
                "{}: include: '{}' needs :fs, which the compiling fiber withholds",
                span, name.path
            ));
        }
        let path = name
            .resolve()
            .ok_or_else(|| format!("{}: include: '{}' not found", span, name.path))?;
        let own = span.source_path();
        if let Some(cycle) = self.cycle(&path, own.as_deref()) {
            return Err(format!("{}: include: circular dependency: {}", span, cycle));
        }
        let text = std::fs::read_to_string(&path)
            .map_err(|e| format!("{}: include: failed to read '{}': {}", span, path, e))?;
        let forms = read_syntax_all_for(*arena, &text, &path)?;
        let mark = ChainMark(self.chain.len());
        if self.chain.is_empty() {
            self.chain.extend(own);
        }
        self.chain.push(path);
        Ok((forms, mark))
    }

    /// Take the files an include entered back off the chain.
    pub(crate) fn leave(&mut self, mark: ChainMark) {
        self.chain.truncate(mark.0);
    }

    /// The cycle an include of `target` from the file `own` closes, written
    /// from the first file being included that is `target`, back to `target`.
    fn cycle(&self, target: &str, own: Option<&str>) -> Option<String> {
        let mut links: Vec<&str> = match self.chain.iter().position(|p| p == target) {
            Some(i) => self.chain[i..].iter().map(String::as_str).collect(),
            None if own == Some(target) => vec![target],
            None => return None,
        };
        links.push(target);
        Some(links.join(" -> "))
    }
}

#[cfg(test)]
mod tests;
