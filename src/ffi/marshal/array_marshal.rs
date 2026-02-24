use super::super::types::CType;
use super::conversions;
use super::cvalue::CValue;
use crate::value::Value;

/// Marshal an array value to C representation.
pub fn marshal_array(value: &Value, elem_type: &CType, _count: usize) -> Result<CValue, String> {
    if let Some(vec_ref) = value.as_array() {
        let mut elements = Vec::new();
        let vec = vec_ref.borrow();
        for elem in vec.iter() {
            elements.push(conversions::elle_to_c(elem, elem_type)?);
        }
        Ok(CValue::Array(elements))
    } else if let Some(cons) = value.as_cons() {
        let mut elements = Vec::new();
        let mut current = Some(cons.clone());
        while let Some(cell) = current {
            elements.push(conversions::elle_to_c(&cell.first, elem_type)?);
            current = cell.rest.as_cons().cloned();
        }
        Ok(CValue::Array(elements))
    } else {
        Err(format!("Cannot marshal {:?} as array", value))
    }
}

/// Unmarshal a C array to Elle value.
pub fn unmarshal_array(cvalue: &CValue, elem_type: &CType) -> Result<Value, String> {
    match cvalue {
        CValue::Array(elements) => {
            let mut result = vec![];
            for elem in elements {
                result.push(conversions::c_to_elle(elem, elem_type)?);
            }
            Ok(Value::array(result))
        }
        _ => Err("Type mismatch in unmarshal: expected array".to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_marshal_array_as_c_array() {
        let val = Value::array(vec![Value::int(1), Value::int(2), Value::int(3)]);
        let cval = marshal_array(&val, &CType::Int, 3).unwrap();
        match cval {
            CValue::Array(elems) => {
                assert_eq!(elems.len(), 3);
                match &elems[0] {
                    CValue::Int(n) => assert_eq!(*n, 1),
                    _ => panic!("Expected Int"),
                }
            }
            _ => panic!("Expected Array"),
        }
    }

    #[test]
    fn test_marshal_cons_as_array() {
        use crate::value::cons;
        let list = cons(
            Value::int(10),
            cons(Value::int(20), cons(Value::int(30), Value::EMPTY_LIST)),
        );
        let cval = marshal_array(&list, &CType::Int, 3).unwrap();
        match cval {
            CValue::Array(elems) => {
                assert_eq!(elems.len(), 3);
            }
            _ => panic!("Expected Array"),
        }
    }

    #[test]
    fn test_unmarshal_c_array_to_array() {
        let cval = CValue::Array(vec![CValue::Int(5), CValue::Int(10), CValue::Int(15)]);
        let val = unmarshal_array(&cval, &CType::Int).unwrap();
        if let Some(vec_ref) = val.as_array() {
            let vec = vec_ref.borrow();
            assert_eq!(vec.len(), 3);
            assert_eq!(vec[0], Value::int(5));
            assert_eq!(vec[1], Value::int(10));
            assert_eq!(vec[2], Value::int(15));
        } else {
            panic!("Expected Array");
        }
    }
}
