use super::super::types::UnionLayout;
use super::cvalue::CValue;
use super::field_packing::{pack_field, unpack_field};
use crate::value::Value;

/// Marshal a union value to C representation with layout information.
///
/// The union value should be an array with a single element: \[field_index_or_value\].
/// The field is packed at offset 0 (all union fields overlap at offset 0).
pub fn marshal_union_with_layout(value: &Value, layout: &UnionLayout) -> Result<CValue, String> {
    if let Some(fields_ref) = value.as_array() {
        let fields = fields_ref.borrow();
        // Union must have exactly one field value
        if fields.is_empty() || fields.len() > layout.fields.len() {
            return Err(format!(
                "Union has {} fields but got {} values",
                layout.fields.len(),
                fields.len()
            ));
        }

        // Get the field index or use first field
        let field_idx = if fields.len() == 2 {
            // Optional: first element could be field index
            if let Some(idx) = fields[0].as_int() {
                if idx < 0 || idx as usize >= layout.fields.len() {
                    return Err(format!("Union field index out of range: {}", idx));
                }
                idx as usize
            } else {
                0 // Treat as [value] format - use first field
            }
        } else {
            0
        };

        let field = &layout.fields[field_idx];
        let field_value = if fields.len() == 2 {
            &fields[1]
        } else {
            &fields[0]
        };

        // Create bytes buffer of union size
        let mut bytes = vec![0u8; layout.size];

        // Pack the field value at offset 0 (all union fields start at 0)
        pack_field(&mut bytes, field_value, 0, &field.ctype)?;

        Ok(CValue::Union(bytes))
    } else {
        Err(format!("Cannot marshal {:?} as union", value))
    }
}

/// Unmarshal a C union to Elle value with layout information.
///
/// Returns an array with all field values (all fields read at offset 0).
/// In practice, the caller must know which field is active.
pub fn unmarshal_union_with_layout(cvalue: &CValue, layout: &UnionLayout) -> Result<Value, String> {
    match cvalue {
        CValue::Union(bytes) => {
            if bytes.len() != layout.size {
                return Err(format!(
                    "Union data size mismatch: expected {}, got {}",
                    layout.size,
                    bytes.len()
                ));
            }

            // Return all field values (they all read the same bytes at offset 0)
            let mut field_values = Vec::new();

            for field in &layout.fields {
                let field_value = unpack_field(bytes, 0, &field.ctype)?;
                field_values.push(field_value);
            }

            Ok(Value::array(field_values))
        }
        _ => Err("Type mismatch in unmarshal: expected union".to_string()),
    }
}
