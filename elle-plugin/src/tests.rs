use super::*;
use std::ffi::c_void;
use std::ptr;

// ── What version 4 names ──────────────────────────────────────────────

/// Every API function version 4 declares, with the signature a v4 plugin
/// compiles its call against.
///
/// This is a transcription of the `elle_api!` block, not a derivation from it:
/// the two are written separately so that changing one and not the other is a
/// failing build. The trap it guards is that `Api::load` resolves by name and
/// then transmutes, so a host that renamed nothing but changed an argument list
/// hands back a pointer the plugin calls with the wrong arguments — a resolve
/// that succeeds into a call that corrupts. Nothing at load time can see that;
/// only `ABI_VERSION` can, and only if it moves.
const V4_FUNCTIONS: &[(&str, &str)] = &[
    ("make_int", "(i64,)->ElleValue"),
    ("make_float", "(f64,)->ElleValue"),
    ("make_bool", "(bool,)->ElleValue"),
    ("make_nil", "()->ElleValue"),
    ("make_string", "(*mut ElleCtx,*const u8,usize,)->ElleValue"),
    ("make_bytes", "(*mut ElleCtx,*const u8,usize,)->ElleValue"),
    ("make_keyword", "(*mut ElleCtx,*const u8,usize,)->ElleValue"),
    ("make_array", "(*mut ElleCtx,*const ElleValue,usize,)->ElleValue"),
    ("make_struct", "(*mut ElleCtx,*const ElleKV,usize,)->ElleValue"),
    ("make_set", "(*mut ElleCtx,*const ElleValue,usize,)->ElleValue"),
    (
        "make_error",
        "(*mut ElleCtx,*const u8,usize,*const u8,usize,)->ElleValue",
    ),
    (
        "make_external",
        "(*mut ElleCtx,*const u8,usize,*mut c_void,Option<extern \"C\" fn(*mut c_void)>,)->ElleValue",
    ),
    ("as_external", "(ElleValue,*const u8,usize,)->*mut c_void"),
    ("as_int", "(ElleValue,*mut i64,)->bool"),
    ("as_float", "(ElleValue,*mut f64,)->bool"),
    ("as_bool", "(ElleValue,)->i32"),
    ("is_nil", "(ElleValue,)->bool"),
    ("is_truthy", "(ElleValue,)->bool"),
    ("as_string", "(ElleValue,*mut usize,)->*const u8"),
    ("as_bytes", "(ElleValue,*mut usize,)->*const u8"),
    ("type_name_of", "(ElleValue,*mut usize,)->*const u8"),
    ("is_string", "(ElleValue,)->bool"),
    ("is_keyword", "(ElleValue,)->bool"),
    ("is_bytes", "(ElleValue,)->bool"),
    ("is_array", "(ElleValue,)->bool"),
    ("is_struct", "(ElleValue,)->bool"),
    ("is_int", "(ElleValue,)->bool"),
    ("is_float", "(ElleValue,)->bool"),
    ("is_bool_val", "(ElleValue,)->bool"),
    ("is_external", "(ElleValue,)->bool"),
    (
        "as_keyword_name",
        "(*mut ElleCtx,ElleValue,*mut usize,)->*const u8",
    ),
    ("struct_get", "(ElleValue,*const u8,usize,)->ElleValue"),
    ("struct_len", "(ElleValue,)->isize"),
    (
        "struct_key",
        "(*mut ElleCtx,ElleValue,usize,*mut usize,)->*const u8",
    ),
    ("struct_value", "(ElleValue,usize,)->ElleValue"),
    ("array_len", "(ElleValue,)->isize"),
    ("array_get", "(ElleValue,usize,)->ElleValue"),
    ("list_to_array", "(*mut ElleCtx,ElleValue,)->ElleValue"),
    ("value_eq", "(ElleValue,ElleValue,)->bool"),
    ("make_poll_fd", "(*mut ElleCtx,i32,u32,)->ElleValue"),
    ("intern_keyword", "(*const u8,usize,)->u64"),
    ("keyword_name", "(*mut ElleCtx,u64,*mut usize,)->*const u8"),
];

// The counter-factual: on the tree that changed `make_keyword`,
// `as_keyword_name`, `struct_key` and `keyword_name` to take the per-call ctx
// and left `ABI_VERSION` at 3, this test is the only thing in either repository
// that fails. Every plugin still compiled, because plugins take `elle-plugin` by
// path and were rebuilt against the new signatures; a plugin `.so` built before
// the change kept loading, because the number the guard compares had not moved.
#[test]
fn every_declared_api_function_has_the_signature_version_4_pinned_it_with() {
    assert_eq!(
        ABI_VERSION, 4,
        "the pin below is version 4's. Move both together, or the number stops \
         naming anything."
    );

    for (name, declared) in ABI_FUNCTIONS {
        let Some((_, pinned)) = V4_FUNCTIONS.iter().find(|(pinned, _)| pinned == name) else {
            panic!(
                "`{name}` is declared in `elle_api!` but is not pinned here. \
                 Adding a function is compatible — a plugin never resolves a name \
                 it was not compiled against — so add the row and leave \
                 ABI_VERSION alone."
            );
        };
        assert_eq!(
            declared, pinned,
            "`{name}` is declared `{declared}`, and version 4 pinned \
             `{pinned}`. A v4 plugin resolves this name and calls it through the \
             pinned argument list, so the load guard sees nothing and the call is \
             wrong. Bump ABI_VERSION, re-pin the set, and add the row to \
             docs/plugins.md § \"The ABI version\"."
        );
    }

    for (name, _) in V4_FUNCTIONS {
        assert!(
            ABI_FUNCTIONS.iter().any(|(declared, _)| declared == name),
            "`{name}` is pinned by version 4 and is no longer declared. A v4 \
             plugin resolves it to null and fails to load with -1, which reads as \
             a corrupt install rather than a version it cannot speak. Bump \
             ABI_VERSION and re-pin."
        );
    }
}

// ── The load guard ────────────────────────────────────────────────────

extern "C" fn unreachable_prim(
    _ctx: *mut ElleCtx,
    _args: *const ElleValue,
    _nargs: usize,
) -> ElleResult {
    unreachable!("these tests never call the registered primitive")
}

static PRIMITIVES: [EllePrimDef; 1] = [EllePrimDef::exact(
    "test/noop",
    unreachable_prim,
    SIG_OK,
    0,
    "doc",
    "test",
    "(test/noop)",
)];

define_plugin!("test/", &PRIMITIVES);

/// Stands in for every API function. `Api::load` only checks for null and
/// transmutes, so one non-null address resolves the whole table.
extern "C" fn stub() {}

extern "C" fn resolve_everything(_name: *const u8, _len: usize) -> *const c_void {
    stub as *const c_void
}

extern "C" fn resolve_nothing(_name: *const u8, _len: usize) -> *const c_void {
    ptr::null()
}

extern "C" fn count_registration(ctx: *mut EllePluginCtx, _def: *const EllePrimDef) {
    let registered = unsafe { (*ctx)._opaque as *mut usize };
    unsafe { *registered += 1 };
}

/// Run `elle_plugin_init` against a host and report `(return code, primitives
/// registered)`.
fn init_against(
    version: u32,
    resolve: extern "C" fn(*const u8, usize) -> *const c_void,
) -> (i32, usize) {
    let loader = ElleApiLoader { version, resolve };
    let mut registered: usize = 0;
    let mut ctx = EllePluginCtx {
        register: count_registration,
        _opaque: &mut registered as *mut usize as *mut c_void,
    };
    let code = elle_plugin_init(&loader, &mut ctx);
    (code, registered)
}

#[test]
fn a_host_one_version_ahead_is_refused_before_a_single_pointer_is_bound() {
    let (code, registered) = init_against(ABI_VERSION + 1, resolve_everything);
    assert_eq!(
        code, -2,
        "a newer host must fail the load, not be called into"
    );
    assert_eq!(
        registered, 0,
        "the guard has to run before registration: a primitive registered here \
         is one elle will call through the wrong convention later."
    );
}

#[test]
fn a_host_one_version_behind_is_refused() {
    let (code, _) = init_against(ABI_VERSION - 1, resolve_everything);
    assert_eq!(
        code, -2,
        "an older host is as unusable as a newer one — the convention differs \
         in both directions."
    );
}

#[test]
fn a_host_on_the_matching_version_loads_and_binds_the_api() {
    let (code, registered) = init_against(ABI_VERSION, resolve_everything);
    assert_eq!(code, 0, "the version the SDK speaks must load");
    assert_eq!(registered, PRIMITIVES.len());
    // `api()` panics when init returned before `API.set`, so reaching this
    // proves the guard let the resolve through rather than short-circuiting.
    let _ = api();
}

#[test]
fn a_host_on_the_matching_version_that_resolves_nothing_is_refused_distinctly() {
    let (code, registered) = init_against(ABI_VERSION, resolve_nothing);
    assert_eq!(
        code, -1,
        "a missing name is -1 and a wrong version is -2; collapsing them would \
         report an unspeakable ABI as a corrupt install."
    );
    assert_eq!(registered, 0);
}
