// audited: 2026-10-05
//! Join every file's facts through the import graph, then lower the call facts for the solver.
//!
//! docs/signals/inference.md
//! docs/modules.md

use crate::extract::{use_fact, ModuleShape, Pending};
use crate::model::{Model, SiteId, VarId, VarKey};
use std::collections::{BTreeMap, BTreeSet, HashMap};

fn owner_of(model: &Model, v: VarId) -> VarId {
    if let VarKey::Sq(_) = model.keys_by_id[v as usize] {
        if let Some(&(_, t)) = model.facts.sqof.iter().find(|(q, _)| *q == v) {
            return owner_of(model, t);
        }
    }
    v
}

/// Emit the facts that need another file's shape: what an instance's fields
/// are, what instantiating a module raises, and whose parameters a call of
/// an export binds.
pub fn link(model: &mut Model, shapes: &HashMap<String, ModuleShape>, pending: Vec<Pending>) {
    let mut instances: BTreeMap<SiteId, (String, bool)> = BTreeMap::new();
    for p in &pending {
        for (site, path, is_ref) in &p.instances {
            instances.insert(*site, (path.clone(), *is_ref));
        }
    }
    let mut read_keys: HashMap<SiteId, Vec<(u32, VarId)>> = HashMap::new();
    for (id, key) in model.keys_by_id.iter().enumerate() {
        if let VarKey::Inst(site, k) = key {
            read_keys.entry(*site).or_default().push((*k, id as VarId));
        }
    }

    // The export variable each instance field stands for, for the export calls.
    let mut export_of: HashMap<VarId, VarId> = HashMap::new();
    let unknown = model.unknown();
    for (site, (path, is_ref)) in &instances {
        let obj = model.var(VarKey::Obj(*site));
        // A module this run did not analyze stays open: each export read from
        // it is a free variable, which a later link would bind. That is a
        // per-file summary.
        let Some(shape) = shapes.get(path).cloned() else {
            for (_, inst) in read_keys.get(site).cloned().unwrap_or_default() {
                model.facts.dep.insert((inst, inst));
                model.facts.owner.insert((inst, inst));
            }
            model.facts.opaque.insert(obj);
            continue;
        };
        let (exports, owner) = match (&shape, is_ref) {
            (ModuleShape::Lambda { lam, exports }, false) => (exports.clone(), *lam),
            (ModuleShape::Struct { exports }, true) => (exports.clone(), obj),
            _ => (Vec::new(), unknown),
        };
        if owner == unknown {
            model.facts.opaque.insert(obj);
        } else {
            model.facts.isobj.insert(obj);
        }
        let nargs = model.site_nargs.get(site).copied().unwrap_or(0);
        let mut seen = Vec::new();
        for (name, x) in &exports {
            let k = model.key(name);
            let inst = model.var(VarKey::Inst(*site, k));
            use_fact(model, inst, *site, *x, owner, nargs);
            model.facts.hasfld.insert((obj, k));
            model.facts.fld.insert((obj, k, inst));
            export_of.insert(inst, *x);
            seen.push(k);
        }
        for (k, inst) in read_keys.get(site).cloned().unwrap_or_default() {
            if !seen.contains(&k) {
                use_fact(model, inst, *site, unknown, unknown, 0);
            }
        }
    }

    for p in pending {
        for (ctx, site, path, nargs) in p.instantiations {
            match shapes.get(&path) {
                Some(ModuleShape::Lambda { lam, .. }) => {
                    model.site_callee.insert(site, *lam);
                    use_fact(model, ctx, site, *lam, *lam, nargs);
                }
                Some(_) => use_fact(model, ctx, site, unknown, unknown, nargs),
                None => {
                    // Instantiating a module this run did not analyze: free.
                    let obj = model.var(VarKey::Obj(site));
                    model.facts.dep.insert((obj, obj));
                    model.facts.owner.insert((obj, obj));
                    use_fact(model, ctx, site, obj, unknown, nargs);
                }
            }
        }
        for call in p.export_calls {
            let owner = export_of
                .get(&call.callee)
                .map(|x| owner_of(model, *x))
                .unwrap_or(unknown);
            use_fact(model, call.ctx, call.site, call.callee, owner, call.nargs);
        }
    }

    close_fields(model);
}

/// The call relations with the site joined away. At a call from `c` to `g`
/// binding the parameters of `o`, a free variable `w` of `o` stands for the
/// argument `a` (`sub`), or for nothing known (`miss`). Everything here is
/// input data, so the joins run once, before the solver.
#[derive(Default)]
pub struct Lowered {
    /// `call(c, g, o)`.
    pub call: BTreeSet<(VarId, VarId, VarId)>,
    /// `sub(c, g, w, a)`.
    pub sub: BTreeSet<(VarId, VarId, VarId, VarId)>,
    /// `miss(c, g, w)`.
    pub miss: BTreeSet<(VarId, VarId, VarId)>,
    /// `flow(c, a)`: c calls an open import with argument a, so a may be
    /// called on c's behalf.
    pub flow: BTreeSet<(VarId, VarId)>,
}

pub fn lower(model: &Model) -> Lowered {
    let f = &model.facts;
    let mut params: HashMap<VarId, Vec<(u32, VarId)>> = HashMap::new();
    for &(w, o, i) in &f.pidx {
        params.entry(o).or_default().push((i, w));
    }
    let mut fields: HashMap<VarId, Vec<(u32, u32, VarId)>> = HashMap::new();
    for &(w, o, i, k) in &f.fidx {
        fields.entry(o).or_default().push((i, k, w));
    }
    let fld: HashMap<(VarId, u32), VarId> = f.fld.iter().map(|&(a, k, v)| ((a, k), v)).collect();
    let mut out = Lowered::default();
    for &(c, site, g, o) in &f.uses {
        out.call.insert((c, g, o));
        if f.owner.contains(&(g, g)) {
            for &(_, _, a) in f.arg.range((site, 0, 0)..(site + 1, 0, 0)) {
                out.flow.insert((c, a));
            }
        }
        let arg = |i: u32| -> Option<VarId> {
            f.arg
                .range((site, i, 0)..=(site, i, VarId::MAX))
                .next()
                .map(|&(_, _, a)| a)
        };
        for &(i, w) in params.get(&o).map(|v| v.as_slice()).unwrap_or(&[]) {
            match arg(i) {
                Some(a) => out.sub.insert((c, g, w, a)),
                None => out.miss.insert((c, g, w)),
            };
        }
        for &(i, k, w) in fields.get(&o).map(|v| v.as_slice()).unwrap_or(&[]) {
            match arg(i).and_then(|a| fld.get(&(a, k)).copied()) {
                Some(fv) => out.sub.insert((c, g, w, fv)),
                None => out.miss.insert((c, g, w)),
            };
        }
    }
    out
}

/// A call that passes a parameter on lets the callee read that parameter's
/// fields, so every field the callee reads needs a variable on the argument.
/// Repeat until no call adds one.
fn close_fields(model: &mut Model) {
    loop {
        let mut fields_of: HashMap<(VarId, u32), Vec<u32>> = HashMap::new();
        for &(_, o, i, k) in &model.facts.fidx {
            fields_of.entry((o, i)).or_default().push(k);
        }
        let mut wanted = Vec::new();
        for &(_, site, _, o) in &model.facts.uses {
            for &(s, i, a) in model.facts.arg.range((site, 0, 0)..(site + 1, 0, 0)) {
                debug_assert_eq!(s, site);
                if !matches!(model.keys_by_id[a as usize], VarKey::Param(..)) {
                    continue;
                }
                for &k in fields_of.get(&(o, i)).map(|v| v.as_slice()).unwrap_or(&[]) {
                    let key = VarKey::Field(a, k);
                    if !model.has(&key) {
                        wanted.push(key);
                    }
                }
            }
        }
        if wanted.is_empty() {
            return;
        }
        for key in wanted {
            model.var(key);
        }
    }
}
