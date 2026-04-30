use super::core::VM;
use crate::value::Value;

pub(crate) fn handle_eq(vm: &mut VM) {
    let b = vm.fiber.stack.pop().expect("VM bug: Stack underflow on Eq");
    let a = vm.fiber.stack.pop().expect("VM bug: Stack underflow on Eq");
    // Fast path: bitwise identical
    if a == b {
        vm.fiber.stack.push(Value::TRUE);
        return;
    }
    // Numeric coercion: int-int stays exact, mixed promotes to f64
    if a.is_number() && b.is_number() {
        if let (Some(x), Some(y)) = (a.as_int(), b.as_int()) {
            vm.fiber
                .stack
                .push(if x == y { Value::TRUE } else { Value::FALSE });
            return;
        }
        if let (Some(x), Some(y)) = (a.as_number(), b.as_number()) {
            vm.fiber
                .stack
                .push(if x == y { Value::TRUE } else { Value::FALSE });
            return;
        }
    }
    vm.fiber.stack.push(Value::FALSE);
}

/// Comparison helper macro. Panics on incomparable types.
macro_rules! cmp_handler {
    ($name:ident, $sym:literal, $int_cmp:expr, $float_cmp:expr, $ord_method:ident) => {
        pub(crate) fn $name(vm: &mut VM) {
            let b = vm
                .fiber
                .stack
                .pop()
                .expect(concat!("VM bug: Stack underflow on ", $sym));
            let a = vm
                .fiber
                .stack
                .pop()
                .expect(concat!("VM bug: Stack underflow on ", $sym));
            let result = match (a.as_int(), b.as_int()) {
                (Some(x), Some(y)) => Value::bool($int_cmp(x, y)),
                _ => match (a.as_number(), b.as_number()) {
                    (Some(x), Some(y)) => Value::bool($float_cmp(x, y)),
                    _ => {
                        if let Some(ord) = a.compare_str(&b) {
                            vm.fiber.stack.push(Value::bool(ord.$ord_method()));
                            return;
                        }
                        if let Some(ord) = a.compare_keyword(&b) {
                            vm.fiber.stack.push(Value::bool(ord.$ord_method()));
                            return;
                        }
                        panic!(
                            concat!(
                                "%",
                                $sym,
                                ": expected number, string, or keyword, got {} and {}"
                            ),
                            a.type_name(),
                            b.type_name()
                        );
                    }
                },
            };
            vm.fiber.stack.push(result);
        }
    };
}

cmp_handler!(
    handle_lt,
    "lt",
    |a: i64, b: i64| a < b,
    |a: f64, b: f64| a < b,
    is_lt
);
cmp_handler!(
    handle_gt,
    "gt",
    |a: i64, b: i64| a > b,
    |a: f64, b: f64| a > b,
    is_gt
);
cmp_handler!(
    handle_le,
    "le",
    |a: i64, b: i64| a <= b,
    |a: f64, b: f64| a <= b,
    is_le
);
cmp_handler!(
    handle_ge,
    "ge",
    |a: i64, b: i64| a >= b,
    |a: f64, b: f64| a >= b,
    is_ge
);
