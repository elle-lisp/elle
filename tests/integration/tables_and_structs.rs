// Integration tests for tables and structs
use elle::compiler::converters::value_to_expr;
use elle::{compile, list, register_primitives, Lexer, Reader, SymbolTable, Value, VM};

fn eval(input: &str) -> Result<Value, String> {
    let mut vm = VM::new();
    let mut symbols = SymbolTable::new();
    register_primitives(&mut vm, &mut symbols);

    let mut lexer = Lexer::new(input);
    let mut tokens = Vec::new();
    while let Some(token) = lexer.next_token()? {
        tokens.push(token);
    }

    if tokens.is_empty() {
        return Err("No input".to_string());
    }

    let mut reader = Reader::new(tokens);
    let mut values = Vec::new();
    while let Some(result) = reader.try_read(&mut symbols) {
        values.push(result?);
    }

    let value = if values.len() == 1 {
        values.into_iter().next().unwrap()
    } else if values.is_empty() {
        return Err("No input".to_string());
    } else {
        let mut begin_args = vec![Value::Symbol(symbols.intern("begin"))];
        begin_args.extend(values);
        list(begin_args)
    };

    let expr = value_to_expr(&value, &mut symbols)?;
    let bytecode = compile(&expr);
    vm.execute(&bytecode)
}

// ============ TABLE TESTS (mutable) ============

#[test]
fn test_table_creation_empty() {
    let result = eval("(table)").unwrap();
    assert!(matches!(result, Value::Table(_)));
}

#[test]
fn test_table_type_name() {
    let result = eval("(table)").unwrap();
    assert_eq!(result.type_name(), "table");
}

// ============ STRUCT TESTS (immutable) ============

#[test]
fn test_struct_creation_empty() {
    let result = eval("(struct)").unwrap();
    assert!(matches!(result, Value::Struct(_)));
}

#[test]
fn test_struct_type_name() {
    let result = eval("(struct)").unwrap();
    assert_eq!(result.type_name(), "struct");
}
