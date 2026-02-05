//! FFI primitive functions for Elle.
//!
//! This module provides Lisp functions for loading and calling C functions.
//! It is organized into logical sub-modules for maintainability.

pub mod callbacks;
pub mod calling;
pub mod context;
pub mod enums;
pub mod handlers;
pub mod library;
pub mod memory;
pub mod types;
pub mod wrappers;

pub use callbacks::{
    prim_free_callback, prim_free_callback_wrapper, prim_make_c_callback,
    prim_make_c_callback_wrapper,
};
pub use calling::{prim_call_c_function, prim_call_c_function_wrapper};
pub use context::{clear_vm_context, get_vm_context, register_ffi_primitives, set_vm_context};
pub use enums::{prim_define_enum, prim_define_enum_wrapper};
pub use handlers::{
    prim_clear_custom_handlers, prim_clear_custom_handlers_wrapper, prim_custom_handler_registered,
    prim_custom_handler_registered_wrapper, prim_define_custom_handler,
    prim_define_custom_handler_wrapper, prim_list_custom_handlers,
    prim_list_custom_handlers_wrapper, prim_unregister_custom_handler,
    prim_unregister_custom_handler_wrapper,
};
pub use library::{
    prim_list_libraries, prim_list_libraries_wrapper, prim_load_library, prim_load_library_wrapper,
};
pub use memory::{
    prim_ffi_last_error, prim_ffi_last_error_wrapper, prim_memory_stats, prim_memory_stats_wrapper,
    prim_null_pointer, prim_null_pointer_wrapper, prim_register_allocation,
    prim_register_allocation_wrapper, prim_type_check, prim_type_check_wrapper,
    prim_with_ffi_safety_checks, prim_with_ffi_safety_checks_wrapper,
};
pub use types::parse_ctype;

// Header parsing is exported from enums module
pub use enums::{prim_load_header_with_lib, prim_load_header_with_lib_wrapper};
