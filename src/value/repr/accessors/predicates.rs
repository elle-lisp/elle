use super::*;

impl Value {
    /// Check if this is a string (immutable heap string).
    #[inline]
    pub fn is_string(&self) -> bool {
        self.tag == TAG_STRING
    }
    /// Check if this is a pair cell.
    #[inline]
    pub fn is_pair(&self) -> bool {
        self.tag == TAG_CONS
    }
    /// Check if this is a mutable @array.
    #[inline]
    pub fn is_array_mut(&self) -> bool {
        self.tag == TAG_ARRAY_MUT
    }
    /// Check if this is a mutable @struct.
    #[inline]
    pub fn is_struct_mut(&self) -> bool {
        self.tag == TAG_STRUCT_MUT
    }
    /// Check if this is an immutable struct.
    #[inline]
    pub fn is_struct(&self) -> bool {
        self.tag == TAG_STRUCT
    }
    /// Check if this is a closure.
    #[inline]
    pub fn is_closure(&self) -> bool {
        self.tag == TAG_CLOSURE
    }
    /// Check if this is a user box (LBox).
    #[inline]
    pub fn is_lbox(&self) -> bool {
        self.tag == TAG_LBOX
    }
    /// Check if this is a compiler capture cell (CaptureCell).
    #[inline]
    pub fn is_capture_cell(&self) -> bool {
        self.tag == TAG_CAPTURE_CELL
    }
    /// Check if this is a fiber.
    #[inline]
    pub fn is_fiber(&self) -> bool {
        self.tag == TAG_FIBER
    }
    /// Check if this is an @string.
    #[inline]
    pub fn is_string_mut(&self) -> bool {
        self.tag == TAG_STRING_MUT
    }
    /// Check if this is a bytes value.
    #[inline]
    pub fn is_bytes(&self) -> bool {
        self.tag == TAG_BYTES
    }
    /// Check if this is an @bytes value.
    #[inline]
    pub fn is_bytes_mut(&self) -> bool {
        self.tag == TAG_BYTES_MUT
    }
    /// Check if this is a syntax object.
    #[inline]
    pub fn is_syntax(&self) -> bool {
        self.tag == TAG_SYNTAX
    }
    /// Check if this is a native function.
    #[inline]
    pub fn is_native_fn(&self) -> bool {
        self.tag == TAG_NATIVE_FN
    }
    /// Check if this is an immutable array.
    #[inline]
    pub fn is_array(&self) -> bool {
        self.tag == TAG_ARRAY
    }
    /// Check if this is an immutable set.
    #[inline]
    pub fn is_set(&self) -> bool {
        self.tag == TAG_SET
    }
    /// Check if this is a mutable set.
    #[inline]
    pub fn is_set_mut(&self) -> bool {
        self.tag == TAG_SET_MUT
    }
    /// Check if this is a parameter.
    #[inline]
    pub fn is_parameter(&self) -> bool {
        self.tag == TAG_PARAMETER
    }
    /// Check if this is a managed pointer.
    #[inline]
    pub fn is_managed_pointer(&self) -> bool {
        self.tag == TAG_MANAGED_PTR
    }
    /// Check if this is an external object.
    #[inline]
    pub fn is_external(&self) -> bool {
        self.tag == TAG_EXTERNAL
    }
    /// Check if this is a thread handle.
    #[inline]
    pub fn is_thread(&self) -> bool {
        self.tag == TAG_THREAD
    }
    /// Check if this is a library handle.
    #[inline]
    pub fn is_lib_handle(&self) -> bool {
        self.tag == TAG_LIB_HANDLE
    }
    /// Check if this is an FFI signature.
    #[inline]
    pub fn is_ffi_sig(&self) -> bool {
        self.tag == TAG_FFI_SIG
    }
    /// Check if this is an FFI type descriptor.
    #[inline]
    pub fn is_ffi_type(&self) -> bool {
        self.tag == TAG_FFI_TYPE
    }
    /// Check if this is a proper list (nil or pair ending in nil).
    pub fn is_list(&self) -> bool {
        let mut current = *self;
        loop {
            if current.is_nil() || current.is_empty_list() {
                return true;
            }
            if let Some(pair) = current.as_pair() {
                current = pair.rest;
            } else {
                return false;
            }
        }
    }
    /// Check if this value is mutable (can be modified in-place).
    #[inline]
    pub fn is_mutable(&self) -> bool {
        self.is_array_mut()
            || self.is_string_mut()
            || self.is_bytes_mut()
            || self.is_struct_mut()
            || self.is_set_mut()
            || self.is_lbox()
            || self.is_capture_cell()
            || self.is_parameter()
    }
}
