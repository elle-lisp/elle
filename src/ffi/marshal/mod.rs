mod array_marshal;
mod conversions;
mod cvalue;
mod field_packing;
mod struct_marshal;
mod union_marshal;

// Re-export public API
pub use conversions::{c_to_elle, elle_to_c};
pub use cvalue::CValue;
pub use struct_marshal::{marshal_struct_with_layout, unmarshal_struct_with_layout};
pub use union_marshal::{marshal_union_with_layout, unmarshal_union_with_layout};

/// Marshals Elle values to C representations.
pub struct Marshal;

impl Marshal {
    /// Convert an Elle value to a C representation.
    pub fn elle_to_c(
        value: &crate::value::Value,
        ctype: &crate::ffi::types::CType,
    ) -> Result<CValue, String> {
        conversions::elle_to_c(value, ctype).map_err(|e| e.to_string())
    }

    /// Marshal a struct value to C representation with layout information.
    pub fn marshal_struct_with_layout(
        value: &crate::value::Value,
        layout: &crate::ffi::types::StructLayout,
    ) -> Result<CValue, String> {
        struct_marshal::marshal_struct_with_layout(value, layout)
    }

    /// Convert a C value back to an Elle value.
    pub fn c_to_elle(
        cvalue: &CValue,
        ctype: &crate::ffi::types::CType,
    ) -> Result<crate::value::Value, String> {
        conversions::c_to_elle(cvalue, ctype).map_err(|e| e.to_string())
    }

    /// Unmarshal a C struct to Elle value with layout information.
    pub fn unmarshal_struct_with_layout(
        cvalue: &CValue,
        layout: &crate::ffi::types::StructLayout,
    ) -> Result<crate::value::Value, String> {
        struct_marshal::unmarshal_struct_with_layout(cvalue, layout)
    }

    /// Marshal a union value to C representation with layout information.
    pub fn marshal_union_with_layout(
        value: &crate::value::Value,
        layout: &crate::ffi::types::UnionLayout,
    ) -> Result<CValue, String> {
        union_marshal::marshal_union_with_layout(value, layout)
    }

    /// Unmarshal a C union to Elle value with layout information.
    pub fn unmarshal_union_with_layout(
        cvalue: &CValue,
        layout: &crate::ffi::types::UnionLayout,
    ) -> Result<crate::value::Value, String> {
        union_marshal::unmarshal_union_with_layout(cvalue, layout)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ffi::types::EnumId;

    #[test]
    fn test_marshal_enum() {
        let val = crate::value::Value::int(5);
        let enum_type = crate::ffi::types::CType::Enum(EnumId::new(1));
        let cval = Marshal::elle_to_c(&val, &enum_type).unwrap();
        match cval {
            CValue::Int(n) => assert_eq!(n, 5),
            _ => panic!("Wrong type"),
        }
    }

    #[test]
    fn test_unmarshal_enum() {
        let cval = CValue::Int(10);
        let enum_type = crate::ffi::types::CType::Enum(EnumId::new(1));
        let val = Marshal::c_to_elle(&cval, &enum_type).unwrap();
        assert_eq!(val, crate::value::Value::int(10));
    }
}
