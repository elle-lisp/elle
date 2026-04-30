use super::core::VM;
use crate::value::Value;

pub(crate) fn handle_is_nil(vm: &mut VM) {
    let val = vm
        .fiber
        .stack
        .pop()
        .expect("VM bug: Stack underflow on IsNil");
    vm.fiber.stack.push(Value::bool(val.is_nil()));
}

pub(crate) fn handle_is_pair(vm: &mut VM) {
    let val = vm
        .fiber
        .stack
        .pop()
        .expect("VM bug: Stack underflow on IsPair");
    vm.fiber.stack.push(Value::bool(val.is_pair()));
}

pub(crate) fn handle_is_number(vm: &mut VM) {
    let val = vm
        .fiber
        .stack
        .pop()
        .expect("VM bug: Stack underflow on IsNumber");
    vm.fiber.stack.push(Value::bool(val.is_number()));
}

pub(crate) fn handle_is_symbol(vm: &mut VM) {
    let val = vm
        .fiber
        .stack
        .pop()
        .expect("VM bug: Stack underflow on IsSymbol");
    vm.fiber.stack.push(Value::bool(val.is_symbol()));
}

pub(crate) fn handle_not(vm: &mut VM) {
    let val = vm
        .fiber
        .stack
        .pop()
        .expect("VM bug: Stack underflow on Not");
    vm.fiber.stack.push(Value::bool(!val.is_truthy()));
}

pub(crate) fn handle_is_array(vm: &mut VM) {
    let val = vm
        .fiber
        .stack
        .pop()
        .expect("VM bug: Stack underflow on IsArray");
    vm.fiber.stack.push(Value::bool(val.is_array()));
}

pub(crate) fn handle_is_array_mut(vm: &mut VM) {
    let val = vm
        .fiber
        .stack
        .pop()
        .expect("VM bug: Stack underflow on IsArrayMut");
    vm.fiber.stack.push(Value::bool(val.is_array_mut()));
}

pub(crate) fn handle_is_struct(vm: &mut VM) {
    let val = vm
        .fiber
        .stack
        .pop()
        .expect("VM bug: Stack underflow on IsStruct");
    vm.fiber.stack.push(Value::bool(val.is_struct()));
}

pub(crate) fn handle_array_len(vm: &mut VM) {
    let val = vm
        .fiber
        .stack
        .pop()
        .expect("VM bug: Stack underflow on ArrayMutLen");
    let len = if let Some(a) = val.as_array_mut() {
        a.borrow().len() as i64
    } else if let Some(t) = val.as_array() {
        t.len() as i64
    } else {
        0
    };
    vm.fiber.stack.push(Value::int(len));
}

pub(crate) fn handle_is_struct_mut(vm: &mut VM) {
    let val = vm
        .fiber
        .stack
        .pop()
        .expect("VM bug: Stack underflow on IsStructMut");
    vm.fiber.stack.push(Value::bool(val.is_struct_mut()));
}

pub(crate) fn handle_is_empty_list(vm: &mut VM) {
    let val = vm
        .fiber
        .stack
        .pop()
        .expect("VM bug: Stack underflow on IsEmptyList");
    vm.fiber.stack.push(Value::bool(val.is_empty_list()));
}

pub(crate) fn handle_is_set(vm: &mut VM) {
    let val = vm
        .fiber
        .stack
        .pop()
        .expect("VM bug: Stack underflow on IsSet");
    vm.fiber.stack.push(Value::bool(val.is_set()));
}

pub(crate) fn handle_is_set_mut(vm: &mut VM) {
    let val = vm
        .fiber
        .stack
        .pop()
        .expect("VM bug: Stack underflow on IsSetMut");
    vm.fiber.stack.push(Value::bool(val.is_set_mut()));
}

pub(crate) fn handle_is_bool(vm: &mut VM) {
    let val = vm
        .fiber
        .stack
        .pop()
        .expect("VM bug: Stack underflow on IsBool");
    vm.fiber.stack.push(Value::bool(val.is_bool()));
}

pub(crate) fn handle_is_int(vm: &mut VM) {
    let val = vm
        .fiber
        .stack
        .pop()
        .expect("VM bug: Stack underflow on IsInt");
    vm.fiber.stack.push(Value::bool(val.is_int()));
}

pub(crate) fn handle_is_float(vm: &mut VM) {
    let val = vm
        .fiber
        .stack
        .pop()
        .expect("VM bug: Stack underflow on IsFloat");
    vm.fiber.stack.push(Value::bool(val.is_float()));
}

pub(crate) fn handle_is_string(vm: &mut VM) {
    let val = vm
        .fiber
        .stack
        .pop()
        .expect("VM bug: Stack underflow on IsString");
    vm.fiber
        .stack
        .push(Value::bool(val.is_string() || val.is_string_mut()));
}

pub(crate) fn handle_is_keyword(vm: &mut VM) {
    let val = vm
        .fiber
        .stack
        .pop()
        .expect("VM bug: Stack underflow on IsKeyword");
    vm.fiber.stack.push(Value::bool(val.is_keyword()));
}

pub(crate) fn handle_is_bytes(vm: &mut VM) {
    let val = vm
        .fiber
        .stack
        .pop()
        .expect("VM bug: Stack underflow on IsBytes");
    vm.fiber
        .stack
        .push(Value::bool(val.is_bytes() || val.is_bytes_mut()));
}

pub(crate) fn handle_is_box(vm: &mut VM) {
    let val = vm
        .fiber
        .stack
        .pop()
        .expect("VM bug: Stack underflow on IsBox");
    vm.fiber.stack.push(Value::bool(val.is_lbox()));
}

pub(crate) fn handle_is_closure(vm: &mut VM) {
    let val = vm
        .fiber
        .stack
        .pop()
        .expect("VM bug: Stack underflow on IsClosure");
    vm.fiber.stack.push(Value::bool(val.is_closure()));
}

pub(crate) fn handle_is_fiber(vm: &mut VM) {
    let val = vm
        .fiber
        .stack
        .pop()
        .expect("VM bug: Stack underflow on IsFiber");
    vm.fiber.stack.push(Value::bool(val.is_fiber()));
}

pub(crate) fn handle_type_of(vm: &mut VM) {
    let val = vm
        .fiber
        .stack
        .pop()
        .expect("VM bug: Stack underflow on TypeOf");
    vm.fiber.stack.push(Value::keyword(val.type_name()));
}

pub(crate) fn handle_length(vm: &mut VM) {
    let val = vm.fiber.stack.pop().expect("VM bug: Stack underflow");
    use unicode_segmentation::UnicodeSegmentation;
    let len = if val.is_empty_list() || val.is_nil() {
        0
    } else if val.is_pair() {
        val.list_to_vec().expect("%length: improper list").len()
    } else if let Some(a) = val.as_array() {
        a.len()
    } else if let Some(a) = val.as_array_mut() {
        a.borrow().len()
    } else if let Some(s) = val.as_struct() {
        s.len()
    } else if let Some(s) = val.as_struct_mut() {
        s.borrow().len()
    } else if let Some(s) = val.as_set() {
        s.len()
    } else if let Some(s) = val.as_set_mut() {
        s.borrow().len()
    } else if let Some(b) = val.as_bytes() {
        b.len()
    } else if let Some(b) = val.as_bytes_mut() {
        b.borrow().len()
    } else if let Some(r) = val.with_string(|s| s.graphemes(true).count()) {
        r
    } else if let Some(buf) = val.as_string_mut() {
        let b = buf.borrow();
        std::str::from_utf8(&b)
            .expect("%length: @string invalid UTF-8")
            .graphemes(true)
            .count()
    } else {
        panic!("%length: unsupported type {}", val.type_name())
    };
    vm.fiber.stack.push(Value::int(len as i64));
}

pub(crate) fn handle_intr_get(vm: &mut VM) {
    use crate::value::TableKey;
    let key = vm.fiber.stack.pop().expect("VM bug: Stack underflow");
    let obj = vm.fiber.stack.pop().expect("VM bug: Stack underflow");
    let result = if let Some(elems) = obj.as_array() {
        let i = key.as_int().expect("%get: array index must be int") as usize;
        elems[i]
    } else if let Some(a) = obj.as_array_mut() {
        let i = key.as_int().expect("%get: @array index must be int") as usize;
        a.borrow()[i]
    } else if let Some(pairs) = obj.as_struct() {
        let tk = TableKey::from_value(&key).expect("%get: unhashable key");
        crate::value::sorted_struct_get(pairs, &tk)
            .copied()
            .unwrap_or(Value::NIL)
    } else if let Some(t) = obj.as_struct_mut() {
        let tk = TableKey::from_value(&key).expect("%get: unhashable key");
        let b = t.borrow();
        b.get(&tk).copied().unwrap_or(Value::NIL)
    } else if let Some(r) = obj.with_string(|s| {
        use unicode_segmentation::UnicodeSegmentation;
        let i = key.as_int().expect("%get: string index must be int") as usize;
        s.graphemes(true)
            .nth(i)
            .map(Value::string)
            .unwrap_or(Value::NIL)
    }) {
        r
    } else {
        panic!("%get: unsupported type {}", obj.type_name())
    };
    vm.fiber.stack.push(result);
}

pub(crate) fn handle_intr_put(vm: &mut VM) {
    let val = vm.fiber.stack.pop().expect("VM bug: Stack underflow");
    let key = vm.fiber.stack.pop().expect("VM bug: Stack underflow");
    let obj = vm.fiber.stack.pop().expect("VM bug: Stack underflow");
    // Delegate to prim_put — it handles all the polymorphic cases.
    // On correct types it never errors; wrong types → panic.
    let (bits, result) = crate::primitives::access::prim_put(&[obj, key, val]);
    assert!(
        !bits.contains(crate::value::SIG_ERROR),
        "%put: type error (intrinsic contract violated)"
    );
    vm.fiber.stack.push(result);
}

pub(crate) fn handle_intr_del(vm: &mut VM) {
    let key = vm.fiber.stack.pop().expect("VM bug: Stack underflow");
    let obj = vm.fiber.stack.pop().expect("VM bug: Stack underflow");
    let (bits, result) = crate::primitives::lstruct::prim_del(&[obj, key]);
    assert!(
        !bits.contains(crate::value::SIG_ERROR),
        "%del: type error (intrinsic contract violated)"
    );
    vm.fiber.stack.push(result);
}

pub(crate) fn handle_intr_has(vm: &mut VM) {
    let key = vm.fiber.stack.pop().expect("VM bug: Stack underflow");
    let obj = vm.fiber.stack.pop().expect("VM bug: Stack underflow");
    let (bits, result) = crate::primitives::lstruct::prim_has_key(&[obj, key]);
    assert!(
        !bits.contains(crate::value::SIG_ERROR),
        "%has?: type error (intrinsic contract violated)"
    );
    vm.fiber.stack.push(result);
}

pub(crate) fn handle_intr_push(vm: &mut VM) {
    let value = vm.fiber.stack.pop().expect("VM bug: Stack underflow");
    let collection = vm.fiber.stack.pop().expect("VM bug: Stack underflow");
    if let Some(vec_ref) = collection.as_array_mut() {
        vec_ref.borrow_mut().push(value);
        vm.fiber.stack.push(collection);
    } else if let Some(elems) = collection.as_array() {
        let mut new = elems.to_vec();
        new.push(value);
        vm.fiber.stack.push(Value::array(new));
    } else {
        panic!("%push: unsupported type {}", collection.type_name())
    }
}

pub(crate) fn handle_intr_pop(vm: &mut VM) {
    let val = vm.fiber.stack.pop().expect("VM bug: Stack underflow");
    let arr = val.as_array_mut().expect("%pop: expected @array");
    let popped = arr.borrow_mut().pop().expect("%pop: empty @array");
    vm.fiber.stack.push(popped);
}

pub(crate) fn handle_intr_freeze(vm: &mut VM) {
    let val = vm.fiber.stack.pop().expect("VM bug: Stack underflow");
    let result = if let Some(a) = val.as_array_mut() {
        Value::array(a.borrow().clone())
    } else if let Some(t) = val.as_struct_mut() {
        let entries: Vec<_> = t.borrow().iter().map(|(k, v)| (k.clone(), *v)).collect();
        Value::struct_from_sorted(entries)
    } else if let Some(s) = val.as_set_mut() {
        Value::set(s.borrow().clone())
    } else if let Some(buf) = val.as_string_mut() {
        let b = buf.borrow();
        let s = std::str::from_utf8(&b).expect("%freeze: @string invalid UTF-8");
        Value::string(s)
    } else if let Some(b) = val.as_bytes_mut() {
        Value::bytes(b.borrow().clone())
    } else {
        // Already immutable — pass through
        val
    };
    vm.fiber.stack.push(result);
}

pub(crate) fn handle_intr_thaw(vm: &mut VM) {
    let val = vm.fiber.stack.pop().expect("VM bug: Stack underflow");
    let result = if let Some(a) = val.as_array() {
        Value::array_mut(a.to_vec())
    } else if let Some(s) = val.as_struct() {
        let entries: std::collections::BTreeMap<_, _> =
            s.iter().map(|(k, v)| (k.clone(), *v)).collect();
        Value::struct_mut_from(entries)
    } else if let Some(s) = val.as_set() {
        Value::set_mut(s.iter().cloned().collect())
    } else if let Some(r) = val.with_string(|s| Value::string_mut(s.as_bytes().to_vec())) {
        r
    } else if let Some(b) = val.as_bytes() {
        Value::bytes_mut(b.to_vec())
    } else {
        // Already mutable — pass through
        val
    };
    vm.fiber.stack.push(result);
}

pub(crate) fn handle_identical(vm: &mut VM) {
    let b = vm
        .fiber
        .stack
        .pop()
        .expect("VM bug: Stack underflow on Identical");
    let a = vm
        .fiber
        .stack
        .pop()
        .expect("VM bug: Stack underflow on Identical");
    // Bitwise tag+payload equality (pointer identity for heap values)
    vm.fiber
        .stack
        .push(Value::bool(a.tag == b.tag && a.payload == b.payload));
}

pub(crate) fn handle_ne(vm: &mut VM) {
    let b = vm.fiber.stack.pop().expect("VM bug: Stack underflow on Ne");
    let a = vm.fiber.stack.pop().expect("VM bug: Stack underflow on Ne");
    // Fast path: bitwise identical → not equal is false
    if a == b {
        vm.fiber.stack.push(Value::FALSE);
        return;
    }
    // Numeric coercion: int-int stays exact, mixed promotes to f64
    if a.is_number() && b.is_number() {
        if let (Some(x), Some(y)) = (a.as_int(), b.as_int()) {
            vm.fiber
                .stack
                .push(if x != y { Value::TRUE } else { Value::FALSE });
            return;
        }
        if let (Some(x), Some(y)) = (a.as_number(), b.as_number()) {
            vm.fiber
                .stack
                .push(if x != y { Value::TRUE } else { Value::FALSE });
            return;
        }
    }
    vm.fiber.stack.push(Value::TRUE);
}

pub(crate) fn handle_bit_not_intr(vm: &mut VM) {
    let val = vm.fiber.stack.pop().expect("VM bug: Stack underflow");
    let n = val.as_int().expect("%bit-not: expected integer");
    vm.fiber.stack.push(Value::int(!n));
}
