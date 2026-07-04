//! Host function registration for the Wasmtime linker.

use wasmtime::*;

use super::host::ElleHost;
use crate::value::repr::TAG_HEAP_START;
use crate::value::Value;

mod dataop;
pub use dataop::*;

mod create;
pub use create::*;
