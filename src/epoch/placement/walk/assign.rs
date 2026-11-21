// audited: 2026-11-20
//! The epoch 17 rule's first walk: a def in a sequence that a read after its form reaches assigns a binding made before the form.
//!
//! docs/epochs/bodies.md
//!
//! The walk places no let, so every def in every sequence outside a body
//! reaches [`Walker::assigning`]. The binding stands directly in the body,
//! before the form the walk is in there, its home. One binding serves every
//! def of its name in one home.

use super::super::reads::Region;
use super::super::{Decl, Def};
use super::run::{after, within};
use super::Walker;
use crate::syntax::Syntax;

impl<'t> Walker<'t> {
    /// Walk the children of `parent` from `from` up to `end`, a sequence that
    /// stands outside a body, and record each def there that assigns.
    pub(super) fn assigning(&mut self, parent: &'t Syntax, from: usize, end: usize) {
        let items = parent.kind.children();
        for (i, item) in items.iter().enumerate().take(end).skip(from) {
            let Some(def) = Def::of(item) else {
                self.at(i, |w| w.form(item));
                continue;
            };
            let region = Region {
                parent,
                first: i,
                end,
            };
            if let Some(name) = self.assignable(def, region) {
                self.assign(def, name, i);
            }
            self.at(i, |w| w.def_parts(def));
        }
    }

    /// The name `def` binds, when it assigns: it binds one name, its body is
    /// no template's, each form around it in its body passes it to the body,
    /// no read before its home or in its home outside `region` reaches it, and
    /// a read after its home does.
    fn assignable(&self, def: Def<'t>, region: Region<'t>) -> Option<&'t str> {
        let [name] = def.names().ok()?[..] else {
            return None;
        };
        let depth = self.bodies.len().checked_sub(1)?;
        let home = &self.bodies[depth];
        if def.binds_pattern() || self.templates.last() == Some(&depth) || home.kept > 0 {
            return None;
        }
        (!within(home, region, &[name]) && after(home, &[name])).then_some(name)
    }

    /// Record that `def`, the child `i` of the current form, assigns `name`,
    /// and declare the binding unless its home already holds one.
    fn assign(&mut self, def: Def<'t>, name: &'t str, i: usize) {
        let seeded = self.bound(name);
        let body = self
            .bodies
            .last_mut()
            .expect("a def that assigns has a body");
        let assigns = self
            .assigns
            .as_mut()
            .expect("the first walk records assigns");
        let mut path = self.path.clone();
        path.push(i);
        assigns.defs.push(path);
        if assigns
            .decls
            .iter()
            .any(|d| d.home == body.home && d.name == name)
        {
            return;
        }
        assigns.decls.push(Decl {
            home: body.home.clone(),
            name: name.to_string(),
            seeded,
            span: def.node.span,
        });
        body.decls.push(name);
    }

    /// Whether a binding of `name` stands at the home of the walk. A body
    /// around it binds the name: the form that opens it, a def before the form
    /// the walk is in there, that form when it is a def of a function, or a
    /// binding this walk declared there.
    fn bound(&self, name: &str) -> bool {
        let defines = |form: &Syntax, function: bool| {
            Def::of(form).is_some_and(|d| {
                (!function || d.value_is_function())
                    && d.names().is_ok_and(|names| names.contains(&name))
            })
        };
        self.bodies.iter().any(|body| {
            body.binds.contains(&name)
                || body.decls.contains(&name)
                || body.forms[..body.at].iter().any(|f| defines(f, false))
                || defines(body.forms[body.at], true)
        })
    }
}
