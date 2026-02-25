//! FFI value types for the Elle runtime

/// FFI library handle
///
/// Wraps a handle ID for a loaded dynamic library.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct LibHandle(pub u32);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lib_handle() {
        let h1 = LibHandle(1);
        let h2 = LibHandle(1);
        let h3 = LibHandle(2);

        assert_eq!(h1, h2);
        assert_ne!(h1, h3);
    }
}
