// audited: 2026-10-22
//! `include` and `include-file`: resolve each to its file, check the chain of files being included, and read it.
//!
//! docs/include.md
//! src/pipeline/AGENTS.md
//!
//! The expander splices what an include reads (src/syntax/expand/include.rs).
//! `splice_includes` splices the top level of source text alone, for the WASM
//! backend.

use super::*;

/// Splice include/include-file directives in source text.
///
/// Reads top-level forms, resolves includes, and returns a single string
/// with all included content inlined. Used by the WASM backend to resolve
/// includes before wrapping user code in ev/run. An `include` resolves on the
/// macro VM of `cctx`, which must have the standard library loaded.
pub fn splice_includes(
    source: &str,
    source_name: &str,
    cctx: &mut CompileCtx,
) -> Result<String, String> {
    // Text in, text out: the trees exist only to be re-rendered, so they get
    // their own heap and die with it.
    let mut home = crate::syntax::SyntaxHeap::new();
    let arena = home.arena();
    let syntaxes = read_syntax_all_for(arena, source, source_name)?;
    let unit = crate::syntax::UnitTag::of(source_name, &syntaxes);
    cctx.with_macro_expansion(arena, Layer::Instance, unit, |vm, _expander, meta| {
        let mut parts: Vec<String> = Vec::new();
        let mut includes = Includes::default();
        splice_text(&arena, syntaxes, vm, &meta, &mut includes, &mut parts)?;
        Ok(parts.join("\n"))
    })
}

/// Render each of `forms` onto `parts`, and in place of an include, the forms
/// of the file it names.
fn splice_text(
    arena: &SyntaxArena,
    forms: Vec<Syntax>,
    vm: &mut crate::vm::VM,
    meta: &crate::primitives::def::PrimitiveMeta,
    includes: &mut Includes,
    parts: &mut Vec<String>,
) -> Result<(), String> {
    for form in forms {
        let Some(name) = IncludeName::of(&form) else {
            parts.push(format!("{}", form));
            continue;
        };
        let (read, mark) = includes.enter(arena, &form, name, vm, meta)?;
        let spliced = splice_text(arena, read, vm, meta, includes, parts);
        includes.leave(mark);
        spliced?;
    }
    Ok(())
}

/// How an include names its file: the spec it wrote, whether the standard
/// library's `import/resolve` resolves it (`include`) or the directory joins
/// it (`include-file`), and the including file's directory. Resolving the name
/// again finds the file a compile would read now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct IncludeName {
    spec: String,
    by_resolver: bool,
    dir: Option<String>,
}

impl IncludeName {
    /// The name `(include "spec")` or `(include-file "path")` writes, or None
    /// when `form` is neither. The directory is that of the file the form was
    /// read from, so an include inside an included file resolves against
    /// that file.
    pub(crate) fn of(form: &Syntax) -> Option<IncludeName> {
        let [head, spec] = form.as_list()? else {
            return None;
        };
        let by_resolver = match head.as_symbol()? {
            "include" => true,
            "include-file" => false,
            _ => return None,
        };
        let SyntaxKind::String(spec) = &spec.kind else {
            return None;
        };
        let dir = form
            .span
            .source_path()
            .and_then(|f| crate::path::parent(&f).map(str::to_string));
        Some(IncludeName {
            spec: spec.to_string(),
            by_resolver,
            dir,
        })
    }

    /// The absolute path of the file this name resolves to, or why it names
    /// none: `import/resolve`'s error for an `include`, and nothing for an
    /// `include-file`, whose join has no error of its own.
    ///
    /// `(include spec)` calls `import/resolve` on `vm`, so that `include` and
    /// `import` share one set of rules (docs/modules.md).
    pub(crate) fn resolve(
        &self,
        vm: &mut crate::vm::VM,
        meta: &crate::primitives::def::PrimitiveMeta,
    ) -> Result<String, Option<String>> {
        if self.by_resolver {
            call_import_resolve(vm, meta, &self.spec, self.dir.as_deref()).map_err(Some)
        } else {
            resolve_include_file(&self.spec, self.dir.as_deref()).ok_or(None)
        }
    }
}

/// One file a compile included: its name, the path the name resolved to, and
/// the text read there.
#[derive(Debug, Clone)]
pub(crate) struct IncludeRead {
    pub(crate) name: IncludeName,
    pub(crate) path: String,
    pub(crate) text: String,
}

impl IncludeRead {
    pub(crate) fn new(name: IncludeName, path: String, text: String) -> IncludeRead {
        IncludeRead { name, path, text }
    }
}

/// The length of the chain before an include entered it, which
/// [`Includes::leave`] restores.
#[must_use]
pub(crate) struct ChainMark(usize);

/// The chain of files being included, which the circular-include check reads,
/// and each include in the order the compile read it.
#[derive(Default)]
pub(crate) struct Includes {
    /// The file that holds the outermost include, then each file an include
    /// entered, outermost first.
    chain: Vec<String>,
    read: Vec<IncludeRead>,
}

impl Includes {
    /// Resolve the include `form`, which names `name`, and read its file's
    /// forms. The file enters the chain until [`leave`](Self::leave) takes the
    /// mark back.
    ///
    /// An include reads a file for the compile, so it reads under the
    /// capabilities of the fiber the compile runs for, as a macro does. A file
    /// already in the chain is a cycle, and so is the file holding `form`.
    pub(crate) fn enter(
        &mut self,
        arena: &SyntaxArena,
        form: &Syntax,
        name: IncludeName,
        vm: &mut crate::vm::VM,
        meta: &crate::primitives::def::PrimitiveMeta,
    ) -> Result<(Vec<Syntax>, ChainMark), String> {
        let span = form.span;
        if vm.fiber.withheld.intersects(crate::signals::SIG_FS) {
            return Err(format!(
                "{}: include: '{}' needs :fs, which the compiling fiber withholds",
                span, name.spec
            ));
        }
        let path = name.resolve(vm, meta).map_err(|why| match why {
            Some(e) => format!("{}: include: '{}' not found: {}", span, name.spec, e),
            None => format!("{}: include: '{}' not found", span, name.spec),
        })?;
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
        self.chain.push(path.clone());
        self.read.push(IncludeRead::new(name, path, text));
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

    /// Each include the compile read, in order, leaving none.
    pub(crate) fn take_read(&mut self) -> Vec<IncludeRead> {
        std::mem::take(&mut self.read)
    }
}

/// Call the standard library's `import/resolve` on `vm`: the absolute path of
/// the file `spec` names, given the including file's directory.
///
/// The arguments are born in a macro allocation scope, which the call closes
/// whatever it answers, as a macro expansion's scope closes
/// (docs/impl/region/rules.md).
fn call_import_resolve(
    vm: &mut crate::vm::VM,
    meta: &crate::primitives::def::PrimitiveMeta,
    spec: &str,
    dir: Option<&str>,
) -> Result<String, String> {
    let resolve = meta
        .functions
        .get(&crate::value::SymbolId::of("import/resolve"))
        .copied()
        .ok_or_else(|| "the standard library's import/resolve is not loaded".to_string())?;
    let heap = unsafe { &mut *vm.heap_ptr };
    let scope = crate::value::arena::begin_macro_scope(heap);
    let region = scope.arg_region();
    let args = [
        crate::value::build::string(heap, spec, region),
        dir.map_or(crate::value::Value::NIL, |d| {
            crate::value::build::string(heap, d, region)
        }),
    ];
    let answer = vm.call_closure(resolve, &args).and_then(|v| {
        v.with_string(|s| s.to_string())
            .ok_or_else(|| format!("import/resolve answered {}", v.type_name()))
    });
    crate::value::arena::reclaim_macro_scope(unsafe { &mut *vm.heap_ptr }, scope);
    answer
}

/// Resolve an include-file path against the including file's directory, or
/// the working directory for code with no file. The answer is absolute and
/// normalized, the spelling the circular-include check keys on.
fn resolve_include_file(spec: &str, dir: Option<&str>) -> Option<String> {
    let joined = match dir {
        Some(dir) => crate::path::join(&[dir, spec]),
        None => spec.to_string(),
    };
    let path = crate::path::absolute(&joined).ok()?;
    crate::path::is_file(&path).then_some(path)
}

#[cfg(test)]
mod tests;
