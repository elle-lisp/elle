//! Re-exports of wrapper functions for context-aware FFI calls.
//!
//! These wrappers allow FFI primitives to be called without requiring a VM reference,
//! using the thread-local VM context instead.

pub use super::callbacks::prim_free_callback_wrapper;
pub use super::callbacks::prim_make_c_callback_wrapper;
pub use super::calling::prim_call_c_function_wrapper;
pub use super::enums::prim_define_enum_wrapper;
pub use super::enums::prim_load_header_with_lib_wrapper;
pub use super::library::prim_list_libraries_wrapper;
pub use super::library::prim_load_library_wrapper;
pub use super::memory::prim_ffi_last_error_wrapper;
pub use super::memory::prim_memory_stats_wrapper;
pub use super::memory::prim_null_pointer_wrapper;
pub use super::memory::prim_register_allocation_wrapper;
pub use super::memory::prim_type_check_wrapper;
pub use super::memory::prim_with_ffi_safety_checks_wrapper;
