use std::ffi::c_void;

/// A value in C representation - raw bytes that can be passed to C functions.
#[derive(Debug, Clone, PartialEq)]
pub enum CValue {
    /// 64-bit integer (covers all scalar integer types on x86-64)
    Int(i64),
    /// 64-bit unsigned integer
    UInt(u64),
    /// 64-bit float (stored as f64)
    Float(f64),
    /// Opaque pointer to C data
    Pointer(*const c_void),
    /// C string (null-terminated)
    String(Vec<u8>),
    /// Raw struct bytes
    Struct(Vec<u8>),
    /// Raw union bytes (all fields at offset 0)
    Union(Vec<u8>),
    /// Array of values
    Array(Vec<CValue>),
}

impl CValue {
    /// Get the raw bytes for this value (for libffi calling).
    pub fn as_raw(&self) -> Vec<u8> {
        match self {
            CValue::Int(n) => n.to_le_bytes().to_vec(),
            CValue::UInt(n) => n.to_le_bytes().to_vec(),
            CValue::Float(f) => f.to_le_bytes().to_vec(),
            CValue::Pointer(p) => (*p as u64).to_le_bytes().to_vec(),
            CValue::String(bytes) => {
                // For C string, return pointer to the data
                let ptr = bytes.as_ptr() as u64;
                ptr.to_le_bytes().to_vec()
            }
            CValue::Struct(bytes) => bytes.clone(),
            CValue::Union(bytes) => bytes.clone(),
            CValue::Array(_) => {
                // Arrays are typically passed by pointer
                vec![]
            }
        }
    }
}
