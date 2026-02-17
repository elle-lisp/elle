// DEFENSE: Integration tests for core primitives
// Tests string, file_io, and registration modules
use elle::compiler::converters::value_to_expr;
use elle::{compile, init_stdlib, read_str, register_primitives, SymbolTable, Value, VM};
use std::fs;

fn eval(input: &str) -> Result<Value, String> {
    let mut vm = VM::new();
    let mut symbols = SymbolTable::new();
    register_primitives(&mut vm, &mut symbols);
    init_stdlib(&mut vm, &mut symbols);

    let value = read_str(input, &mut symbols)?;
    let expr = value_to_expr(&value, &mut symbols)?;
    let bytecode = compile(&expr);
    vm.execute(&bytecode)
}

// ============================================================================
// SECTION 1: String Primitive Tests
// ============================================================================
// Tests for src/primitives/string.rs

#[test]
fn test_string_length_basic() {
    // Basic string length
    let result = eval("(length \"hello\")");
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), Value::int(5));
}

#[test]
fn test_string_length_empty() {
    // Empty string length
    let result = eval("(length \"\")");
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), Value::int(0));
}

#[test]
fn test_string_length_unicode() {
    // Unicode characters (emoji is 1 character, 4 bytes in UTF-8)
    let result = eval("(length \"hello🌍\")");
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), Value::int(6)); // 5 ASCII characters + 1 emoji character
}

#[test]
fn test_string_append_multiple() {
    // String append with multiple arguments
    let result = eval("(string-append \"hello\" \" \" \"world\")");
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), Value::string("hello world"));
}

#[test]
fn test_string_append_empty() {
    // String append with empty strings
    let result = eval("(string-append \"\" \"test\" \"\")");
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), Value::string("test"));
}

#[test]
fn test_string_upcase() {
    // Convert to uppercase
    let result = eval("(string-upcase \"Hello World\")");
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), Value::string("HELLO WORLD"));
}

#[test]
fn test_string_downcase() {
    // Convert to lowercase
    let result = eval("(string-downcase \"Hello World\")");
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), Value::string("hello world"));
}

#[test]
fn test_substring_basic() {
    // Basic substring
    let result = eval("(substring \"hello\" 1 4)");
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), Value::string("ell"));
}

#[test]
fn test_substring_from_start() {
    // Substring from start
    let result = eval("(substring \"hello\" 0 3)");
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), Value::string("hel"));
}

#[test]
fn test_string_replace_basic() {
    // String replace
    let result = eval("(string-replace \"hello world\" \"world\" \"universe\")");
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), Value::string("hello universe"));
}

#[test]
fn test_string_split_basic() {
    // Split string
    let result = eval("(string-split \"a,b,c\" \",\")");
    assert!(result.is_ok());
}

#[test]
fn test_string_join() {
    // Join strings
    let result = eval("(string-join '(\"a\" \"b\" \"c\") \",\")");
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), Value::string("a,b,c"));
}

#[test]
fn test_string_trim_spaces() {
    // Trim whitespace
    let result = eval("(string-trim \"  hello  \")");
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), Value::string("hello"));
}

#[test]
fn test_number_to_string() {
    // Convert number to string
    let result = eval("(number->string 42)");
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), Value::string("42"));
}

// ============================================================================
// SECTION 2: File I/O Primitive Tests
// ============================================================================
// Tests for src/primitives/file_io.rs

#[test]
fn test_file_extension() {
    // Get file extension
    let result = eval("(file-extension \"test.txt\")");
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), Value::string("txt"));
}

#[test]
fn test_file_name() {
    // Get file name from path
    let result = eval("(file-name \"path/to/file.txt\")");
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), Value::string("file.txt"));
}

#[test]
fn test_current_directory() {
    // Get current directory
    let result = eval("(current-directory)");
    assert!(result.is_ok());
    assert!((result.unwrap()).is_string());
}

#[test]
fn test_join_path() {
    // Join path components
    let result = eval("(join-path \"path\" \"to\" \"file\")");
    assert!(result.is_ok());
}

#[test]
fn test_absolute_path() {
    // Get absolute path
    let result = eval("(absolute-path \".\")");
    assert!(result.is_ok());
    assert!((result.unwrap()).is_string());
}

#[test]
fn test_file_size_known_file() {
    // Get file size of known file
    let result = eval("(file-size \"Cargo.toml\")");
    assert!(result.is_ok());
    assert!((result.unwrap()).is_int());
}

#[test]
fn test_file_io_slurp_write_basic() {
    // Test file write and read - uses a temp file
    let test_dir = std::env::temp_dir();
    let test_file = test_dir.join("test_slurp.txt");
    let test_file_str = test_file.to_string_lossy();

    // Clean up any existing test file
    let _ = fs::remove_file(&test_file);

    // Write file
    let write_result = eval(&format!("(spit \"{}\" \"hello world\")", test_file_str));
    assert!(write_result.is_ok());

    // Read file
    let read_result = eval(&format!("(slurp \"{}\")", test_file_str));
    assert!(read_result.is_ok());
    assert_eq!(read_result.unwrap(), Value::string("hello world"));

    // Clean up
    let _ = fs::remove_file(&test_file);
}

#[test]
fn test_file_io_append() {
    // Test file append
    let test_dir = std::env::temp_dir();
    let test_file = test_dir.join("test_append.txt");
    let test_file_str = test_file.to_string_lossy();

    // Clean up any existing test file
    let _ = fs::remove_file(&test_file);

    // Write initial content
    let _ = eval(&format!("(spit \"{}\" \"hello\")", test_file_str));

    // Append content
    let append_result = eval(&format!("(append-file \"{}\" \" world\")", test_file_str));
    assert!(append_result.is_ok());

    // Verify appended content
    let read_result = eval(&format!("(slurp \"{}\")", test_file_str));
    assert!(read_result.is_ok());
    assert_eq!(read_result.unwrap(), Value::string("hello world"));

    // Clean up
    let _ = fs::remove_file(&test_file);
}

#[test]
fn test_list_directory() {
    // List directory contents
    let result = eval("(list-directory \"src\")");
    assert!(result.is_ok());
}

#[test]
fn test_parent_directory() {
    // Get parent directory
    let result = eval("(parent-directory \"path/to/file.txt\")");
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), Value::string("path/to"));
}

#[test]
fn test_read_lines() {
    // Create a file with multiple lines and read them
    let test_dir = std::env::temp_dir();
    let test_file = test_dir.join("test_lines.txt");
    let test_file_str = test_file.to_string_lossy();

    // Clean up
    let _ = fs::remove_file(&test_file);

    // Write file with multiple lines
    let _ = eval(&format!(
        "(spit \"{}\" \"Line1\\nLine2\\nLine3\")",
        test_file_str
    ));

    // Read lines
    let result = eval(&format!("(read-lines \"{}\")", test_file_str));
    assert!(result.is_ok());

    // Clean up
    let _ = fs::remove_file(&test_file);
}

// ============================================================================
// SECTION 3: Registration and Module Tests
// ============================================================================
// Tests for src/primitives/registration.rs and module_init.rs

#[test]
fn test_arithmetic_primitives_registered() {
    // Test that arithmetic primitives are available
    let result = eval("(+ 1 2 3)");
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), Value::int(6));
}

#[test]
fn test_string_primitives_registered() {
    // Test that string primitives are available
    let result = eval("(length \"test\")");
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), Value::int(4));
}

#[test]
fn test_list_primitives_registered() {
    // Test that list primitives are available
    let result = eval("(length '(1 2 3))");
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), Value::int(3));
}

#[test]
fn test_comparison_primitives_registered() {
    // Test that comparison primitives are available
    let result = eval("(> 5 3)");
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), Value::bool(true));
}

#[test]
fn test_type_check_primitives_registered() {
    // Test that type check primitives are available
    let result = eval("(string? \"test\")");
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), Value::bool(true));
}

#[test]
fn test_multiple_stdlib_features() {
    // Test that multiple stdlib features work together
    let result = eval(
        "(begin \
         (define greeting \"hello\") \
         (string-append greeting \" world\"))",
    );
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), Value::string("hello world"));
}

#[test]
fn test_higher_order_with_strings() {
    // Test higher-order functions with lambdas on strings
    let result = eval(
        "(map (lambda (s) (string-upcase s)) \
         '(\"hello\" \"world\" \"test\"))",
    );
    assert!(result.is_ok());
}

#[test]
fn test_map_with_primitives() {
    // Test map with string primitives
    let result = eval("(map (lambda (x) (number->string x)) '(1 2 3 4 5))");
    assert!(result.is_ok());
}

#[test]
fn test_fold_with_string_append() {
    // Test fold with string append
    let result = eval(
        "(fold (lambda (acc x) \
         (string-append acc (number->string x))) \
         \"\" '(1 2 3))",
    );
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), Value::string("123"));
}

#[test]
fn test_string_operations_combined() {
    // Test combined string operations
    let result = eval(
        "(begin \
         (define text \"hello\") \
         (string-append (string-upcase text) \" WORLD\"))",
    );
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), Value::string("HELLO WORLD"));
}

#[test]
fn test_file_text_processing() {
    // File reading and text processing
    let test_dir = std::env::temp_dir();
    let test_file = test_dir.join("test_processing.txt");
    let test_file_str = test_file.to_string_lossy();

    // Clean up
    let _ = fs::remove_file(&test_file);

    // Create test file
    let _ = eval(&format!(
        "(spit \"{}\" \"Line1\\nLine2\\nLine3\")",
        test_file_str
    ));

    // Read and process
    let result = eval(&format!(
        "(begin \
         (define content (slurp \"{}\")) \
         (string-upcase content))",
        test_file_str
    ));
    assert!(result.is_ok());

    // Clean up
    let _ = fs::remove_file(&test_file);
}

// ============================================================================
// SECTION 4: Integration Tests
// ============================================================================

#[test]
fn test_string_pipeline() {
    // Complete string manipulation pipeline
    let result = eval(
        "(begin \
         (define text \"hello world\") \
         (define words (string-split text \" \")) \
         (map string-upcase words))",
    );
    assert!(result.is_ok());
}

#[test]
fn test_file_io_with_strings() {
    // Test file I/O with string operations
    let test_dir = std::env::temp_dir();
    let test_file = test_dir.join("test_string_io.txt");
    let test_file_str = test_file.to_string_lossy();

    // Clean up
    let _ = fs::remove_file(&test_file);

    // Write formatted string to file
    let write_result = eval(&format!(
        "(spit \"{}\" (string-upcase \"hello world\"))",
        test_file_str
    ));
    assert!(write_result.is_ok());

    // Read and verify
    let read_result = eval(&format!("(slurp \"{}\")", test_file_str));
    assert!(read_result.is_ok());
    assert_eq!(read_result.unwrap(), Value::string("HELLO WORLD"));

    // Clean up
    let _ = fs::remove_file(&test_file);
}
