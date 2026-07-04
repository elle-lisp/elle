//! FFI value types for the Elle runtime

/// FFI library handle
///
/// Wraps a handle ID for a loaded dynamic library.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct LibHandle(pub u32);

#[cfg(test)]
mod tests;
