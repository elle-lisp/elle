// audited: 2026-11-10
//! A file's spliced top-level `begin`, the `def` forms nested in its forms, and the lambdas its fixpoint re-analyzes.
//!
//! docs/bindings.md
//! docs/pipeline.md

use super::*;
use crate::hir::analyze::owned::reach;
use std::rc::Rc;

/// The file's scope as one lambda saw it when Pass 2 analyzed it. Consecutive
/// lambdas share a view until the file registers another name.
type ScopeView = Rc<HashMap<SymbolId, Vec<ScopedBinding>>>;

/// The lambdas a file's fixpoint re-analyzes, in the order Pass 2 analyzed
/// them, while the file analyzes its forms.
pub(in crate::hir::analyze) struct FileLambdas {
    /// The index of the file's scope in the analyzer's scopes.
    depth: usize,
    lambdas: Vec<FileLambda>,
    /// The last view taken, and the scope's local count when it was taken.
    last_view: Option<(u16, ScopeView)>,
}

/// One lambda the file's fixpoint re-analyzes, with the scope it saw.
struct FileLambda {
    binding: Binding,
    source: Source,
    view: ScopeView,
}

/// Where a lambda the fixpoint re-analyzes stands in the file.
enum Source {
    /// A top-level form's value: its index in the syntax Pass 2 collected, and
    /// its entry's index in the letrec's bindings.
    Entry { syntax: usize, at: usize },
    /// The value of a `def` nested in a top-level form. The `Define` of its
    /// binding takes the converged value.
    Nested(Syntax),
}

impl FileLambdas {
    pub(super) fn new(depth: usize) -> Self {
        FileLambdas {
            depth,
            lambdas: Vec::new(),
            last_view: None,
        }
    }

    /// The file's scope as it stands, shared with the last view while the file
    /// has registered no name since.
    fn view(&mut self, scope: &Scope) -> ScopeView {
        if let Some((at, view)) = &self.last_view {
            if *at == scope.next_local {
                return view.clone();
            }
        }
        let view = Rc::new(scope.bindings.clone());
        self.last_view = Some((scope.next_local, view.clone()));
        view
    }
}

/// Replace the value of each `Define` under `hir` whose binding `values` holds.
fn replace_defines(hir: &mut Hir, values: &mut HashMap<Binding, Hir>) {
    if values.is_empty() {
        return;
    }
    if let HirKind::Define { binding, value } = &mut hir.kind {
        if let Some(converged) = values.remove(binding) {
            **value = converged;
            return;
        }
    }
    hir.for_each_child_mut(|child| replace_defines(child, values));
}

/// The forms of `forms`, each top-level `begin` replaced by its own forms.
/// `begin` opens no scope, so what it holds stands at top level.
pub(super) fn splice_begins(forms: Vec<FileForm<'_>>) -> Vec<FileForm<'_>> {
    let mut out = Vec::with_capacity(forms.len());
    for form in forms {
        splice_into(form, &mut out);
    }
    out
}

fn splice_into<'s>(form: FileForm<'s>, out: &mut Vec<FileForm<'s>>) {
    if let FileForm::Expr(syntax) = &form {
        if let Some(items) = syntax.as_list() {
            if items.len() > 1 && matches!(items[0].as_symbol(), Some("begin" | "do")) {
                for item in &items[1..] {
                    splice_into(classify_form(item), out);
                }
                return;
            }
        }
    }
    out.push(form);
}

impl<'a> Analyzer<'a> {
    /// Pass 1 helper: declare each `def` nested in the top-level form
    /// `syntax`, and pre-bind the first `def` of each name the file has not
    /// bound, for the file `owner`. The analysis of the `def` binds a later
    /// one, visible after its value.
    pub(super) fn prebind_nested(
        &mut self,
        syntax: &Syntax,
        seen_names: &mut HashSet<String>,
        owner: usize,
    ) -> Result<(), String> {
        let mut reached = Vec::new();
        reach(syntax, false, &mut reached);
        seen_names.extend(self.forward_defs(reached, owner)?);
        Ok(())
    }

    /// Pass 2: record the lambda a top-level form's value analyzed to. Its
    /// syntax is `syntax` in the list Pass 2 collects, and its entry stands at
    /// `at` in the letrec's bindings.
    pub(super) fn entry_lambda(&mut self, binding: Binding, syntax: usize, at: usize) {
        self.push_file_lambda(binding, Source::Entry { syntax, at });
    }

    fn push_file_lambda(&mut self, binding: Binding, source: Source) {
        let (Some(file), Some(scope)) = (self.defs.file.as_mut(), self.scopes.last()) else {
            return;
        };
        let view = file.view(scope);
        file.lambdas.push(FileLambda {
            binding,
            source,
            view,
        });
    }

    /// Whether a `def` of the value `value` stands in the scope of a file
    /// whose forms are being analyzed, and the value is a lambda.
    pub(in crate::hir::analyze) fn defines_file_lambda(&self, value: &Syntax) -> bool {
        let is_fn = value.as_list().and_then(|items| items.first()?.as_symbol()) == Some("fn");
        is_fn
            && self
                .defs
                .file
                .as_ref()
                .is_some_and(|file| file.depth + 1 == self.scopes.len())
    }

    /// Pass 2: record the lambda `value`, which a `def` nested in a top-level
    /// form binds to `binding`, with the scope as the `def` saw it. Its
    /// callers read its signal as they read a top-level lambda's.
    pub(in crate::hir::analyze) fn nested_lambda(&mut self, binding: Binding, value: &Syntax) {
        self.push_file_lambda(binding, Source::Nested(*value));
    }

    /// Pass 3: converge the signals of the lambdas `file` holds, each
    /// re-analyzed against the scope it saw, and put each value in its place.
    /// `syntax` is what Pass 2 collected for the top-level lambdas.
    pub(super) fn converge_file_lambdas(
        &mut self,
        file: Option<FileLambdas>,
        syntax: &[&Syntax],
        bindings: &mut [(Binding, Hir)],
    ) -> Result<(), String> {
        let Some(file) = file.filter(|file| !file.lambdas.is_empty()) else {
            return Ok(());
        };
        let entries: Vec<(Binding, &Syntax)> = file
            .lambdas
            .iter()
            .map(|lambda| match &lambda.source {
                Source::Entry { syntax: index, .. } => (lambda.binding, syntax[*index]),
                Source::Nested(value) => (lambda.binding, value),
            })
            .collect();
        let converged = self.converge_in_views(&entries, &file.lambdas)?;
        let mut nested = HashMap::new();
        for (lambda, value) in file.lambdas.iter().zip(converged) {
            match lambda.source {
                Source::Entry { at, .. } => bindings[at].1 = value,
                Source::Nested(_) => {
                    nested.insert(lambda.binding, value);
                }
            }
        }
        for (_, value) in bindings.iter_mut() {
            replace_defines(value, &mut nested);
        }
        Ok(())
    }

    /// Converge the signals of `entries`, each re-analyzed against the view
    /// its lambda in `lambdas` holds, then restore the scope Pass 2 left.
    fn converge_in_views(
        &mut self,
        entries: &[(Binding, &Syntax)],
        lambdas: &[FileLambda],
    ) -> Result<Vec<Hir>, String> {
        let saved = self.scopes.last().map(|scope| scope.bindings.clone());
        let mut installed = None;
        let converged = self.converge_signals_in(entries, |analyzer, i| {
            let view = &lambdas[i].view;
            if installed != Some(Rc::as_ptr(view)) {
                if let Some(scope) = analyzer.scopes.last_mut() {
                    scope.bindings = (**view).clone();
                }
                installed = Some(Rc::as_ptr(view));
            }
        });
        if let (Some(saved), Some(scope)) = (saved, self.scopes.last_mut()) {
            scope.bindings = saved;
        }
        converged
    }

    /// After Pass 2 analyzed the entry that starts at `entry_start`: each
    /// nested `def` a read reached before it is a file binding set before the
    /// entry, and each merged binding the entry left waiting is a file binding
    /// defined after it. Nothing reads the last entry's merged bindings.
    pub(super) fn settle_file_entry(
        &mut self,
        owner: usize,
        entry_start: usize,
        last: bool,
        bindings: &mut Vec<(Binding, Hir)>,
    ) {
        let mut frame = std::mem::take(&mut self.defs.owners[owner]);
        let hoisted = self.take_hoisted(&mut frame);
        let shift = hoisted.len();
        bindings.splice(entry_start..entry_start, hoisted);
        let lambdas = self.defs.file.iter_mut().flat_map(|f| f.lambdas.iter_mut());
        for lambda in lambdas {
            if let Source::Entry { ref mut at, .. } = lambda.source {
                if *at >= entry_start {
                    *at += shift;
                }
            }
        }
        if !last {
            bindings.extend(self.take_merged_entries());
        }
    }
}
