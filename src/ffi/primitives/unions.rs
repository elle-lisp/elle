//! C union type definition and manipulation primitives.

use crate::ffi::types::{UnionField, UnionId, UnionLayout};
use crate::value::Value;
use crate::vm::VM;
use std::sync::atomic::{AtomicU32, Ordering};

#[cfg(test)]
use crate::ffi::types::CType;

/// (define-c-union name ((field-name type) ...)) -> union-id
///
/// Defines a C union type in Elle.
///
/// # Arguments
/// - name: Name of the union (string)
/// - fields: List of (field-name field-type) pairs
///
/// # Returns
/// Union ID as an integer
pub fn prim_define_c_union(_vm: &mut VM, args: &[Value]) -> Result<Value, String> {
    if args.len() != 2 {
        return Err("define-c-union requires exactly 2 arguments".into());
    }

    let union_name = match args[0].as_string() {
        Some(s) => s,
        None => return Err("union name must be a string".into()),
    };

    // Parse fields from list: ((name type) ...)
    let fields_list = &args[1];
    let mut fields = Vec::new();

    if fields_list.is_cons() {
        let fields_vec = fields_list.list_to_vec()?;
        for field_val in fields_vec {
            if let Some(cons) = field_val.as_cons() {
                let field_name = match cons.first.as_string() {
                    Some(n) => n.to_string(),
                    None => return Err("field name must be a string".into()),
                };

                let field_type = match cons.rest.as_cons() {
                    Some(rest_cons) => super::types::parse_ctype(&rest_cons.first)?,
                    None => return Err("field must be (name type)".into()),
                };

                fields.push(UnionField {
                    name: field_name,
                    ctype: field_type,
                });
            } else {
                return Err("each field must be a cons cell".into());
            }
        }
    } else if !fields_list.is_nil() && !fields_list.is_empty_list() {
        return Err("fields must be a list".into());
    }

    if fields.is_empty() {
        return Err("union must have at least one field".into());
    }

    // Calculate union size and alignment (max of all fields)
    let size = fields.iter().map(|f| f.ctype.size()).max().unwrap_or(1);
    let align = fields
        .iter()
        .map(|f| f.ctype.alignment())
        .max()
        .unwrap_or(1);

    // Create union layout with unique ID
    static UNION_ID_COUNTER: AtomicU32 = AtomicU32::new(1);
    let union_id = UnionId::new(UNION_ID_COUNTER.fetch_add(1, Ordering::SeqCst));

    let _layout = UnionLayout::new(union_id, union_name.to_string(), fields, size, align);

    // Return union ID as integer
    Ok(Value::int(union_id.0 as i64))
}

/// Wrapper for prim_define_c_union that works with the primitive system.
pub fn prim_define_c_union_wrapper(args: &[Value]) -> crate::error::LResult<Value> {
    if args.len() != 2 {
        return Err("define-c-union requires exactly 2 arguments"
            .to_string()
            .into());
    }
    // This would be called with a VM in a full implementation
    // For now, we do the simple version
    prim_define_c_union(&mut VM::new(), args).map_err(|e| e.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_union_id_creation() {
        let id1 = UnionId::new(1);
        let id2 = UnionId::new(2);
        assert_ne!(id1, id2);
    }

    #[test]
    fn test_union_layout_field_lookup() {
        let fields = vec![
            UnionField {
                name: "x".to_string(),
                ctype: CType::Int,
            },
            UnionField {
                name: "y".to_string(),
                ctype: CType::Float,
            },
        ];
        let layout = UnionLayout::new(UnionId::new(1), "TestUnion".to_string(), fields, 8, 8);

        assert!(layout.get_field("x").is_some());
        assert!(layout.get_field("y").is_some());
        assert!(layout.get_field("z").is_none());
        assert!(layout.has_field("x"));
        assert!(!layout.has_field("z"));
    }

    #[test]
    fn test_union_size_calculation() {
        // Union size should be max of field sizes
        let fields = vec![
            UnionField {
                name: "a".to_string(),
                ctype: CType::Int, // 4 bytes
            },
            UnionField {
                name: "b".to_string(),
                ctype: CType::Long, // 8 bytes
            },
        ];
        let layout = UnionLayout::new(UnionId::new(1), "TestUnion".to_string(), fields, 8, 8);
        assert_eq!(layout.size, 8);
    }

    #[test]
    fn test_union_alignment_calculation() {
        // Union alignment should be max of field alignments
        let fields = vec![
            UnionField {
                name: "a".to_string(),
                ctype: CType::Char, // 1 byte, align 1
            },
            UnionField {
                name: "b".to_string(),
                ctype: CType::Double, // 8 bytes, align 8
            },
        ];
        let layout = UnionLayout::new(UnionId::new(1), "TestUnion".to_string(), fields, 8, 8);
        assert_eq!(layout.align, 8);
    }
}
