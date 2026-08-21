//! Unit tests (`super` is the parent impl module).

use super::*;

fn lex_single(input: &str) -> Token<'_> {
    let mut lexer = Lexer::new(input);
    lexer.next_token_with_loc().unwrap().unwrap().token
}

fn lex_err(input: &str) -> String {
    let mut lexer = Lexer::new(input);
    lexer.next_token_with_loc().unwrap_err()
}

// ---- Radix contract ----
//
// These pin the type directly so the radix value and its accepted digit set
// can't drift apart: every char Radix::is_digit accepts must be a digit that
// from_str_radix accepts at that base, and vice-versa.

#[test]
fn radix_prefix_chars_map_case_insensitively() {
    assert_eq!(Radix::from_prefix_char('x'), Some(Radix::Hexadecimal));
    assert_eq!(Radix::from_prefix_char('X'), Some(Radix::Hexadecimal));
    assert_eq!(Radix::from_prefix_char('o'), Some(Radix::Octal));
    assert_eq!(Radix::from_prefix_char('B'), Some(Radix::Binary));
    assert_eq!(Radix::from_prefix_char('z'), None);
    assert_eq!(Radix::from_prefix_char('d'), None);
}

#[test]
fn radix_value_and_name_match_each_base() {
    assert_eq!((Radix::Binary.value(), Radix::Binary.name()), (2, "binary"));
    assert_eq!((Radix::Octal.value(), Radix::Octal.name()), (8, "octal"));
    assert_eq!(
        (Radix::Decimal.value(), Radix::Decimal.name()),
        (10, "decimal")
    );
    assert_eq!(
        (Radix::Hexadecimal.value(), Radix::Hexadecimal.name()),
        (16, "hexadecimal")
    );
}

#[test]
fn radix_digit_set_agrees_with_its_value() {
    // For each base, a char is accepted by is_digit iff from_str_radix
    // accepts it as a single digit at that base.
    for radix in [
        Radix::Binary,
        Radix::Octal,
        Radix::Decimal,
        Radix::Hexadecimal,
    ] {
        for c in "0123456789abcdefABCDEFgz".chars() {
            let parses = i64::from_str_radix(&c.to_string(), radix.value()).is_ok();
            assert_eq!(
                radix.is_digit(c),
                parses,
                "{radix:?}.is_digit({c:?}) disagreed with from_str_radix base {}",
                radix.value()
            );
        }
    }
}

#[test]
fn only_decimal_is_unprefixed() {
    assert!(!Radix::Decimal.is_prefixed());
    assert!(Radix::Binary.is_prefixed());
    assert!(Radix::Octal.is_prefixed());
    assert!(Radix::Hexadecimal.is_prefixed());
}

// ---- Hex literals ----

#[test]
fn hex_lowercase_prefix() {
    assert!(matches!(lex_single("0xff"), Token::Integer(255)));
}

#[test]
fn hex_uppercase_prefix() {
    assert!(matches!(lex_single("0XFF"), Token::Integer(255)));
}

#[test]
fn hex_mixed_case_digits() {
    assert!(matches!(lex_single("0x1A2b"), Token::Integer(0x1A2B)));
}

#[test]
fn hex_zero() {
    assert!(matches!(lex_single("0x0"), Token::Integer(0)));
}

#[test]
fn hex_max_positive() {
    // 0x7FFFFFFFFFFFFFFF == i64::MAX
    assert!(matches!(
        lex_single("0x7FFFFFFFFFFFFFFF"),
        Token::Integer(i64::MAX)
    ));
}

#[test]
fn hex_with_underscore() {
    assert!(matches!(lex_single("0xFF_FF"), Token::Integer(0xFFFF)));
}

#[test]
fn hex_positive_sign() {
    assert!(matches!(lex_single("+0xFF"), Token::Integer(255)));
}

// ---- Octal literals ----

#[test]
fn octal_lowercase_prefix() {
    assert!(matches!(lex_single("0o755"), Token::Integer(493)));
}

#[test]
fn octal_uppercase_prefix() {
    assert!(matches!(lex_single("0O755"), Token::Integer(493)));
}

#[test]
fn octal_zero() {
    assert!(matches!(lex_single("0o0"), Token::Integer(0)));
}

#[test]
fn octal_with_underscore() {
    assert!(matches!(lex_single("0o7_5_5"), Token::Integer(493)));
}

// ---- Binary literals ----

#[test]
fn binary_lowercase_prefix() {
    assert!(matches!(lex_single("0b1010"), Token::Integer(10)));
}

#[test]
fn binary_uppercase_prefix() {
    assert!(matches!(lex_single("0B1010"), Token::Integer(10)));
}

#[test]
fn binary_zero() {
    assert!(matches!(lex_single("0b0"), Token::Integer(0)));
}

#[test]
fn binary_with_underscore() {
    assert!(matches!(
        lex_single("0b1010_1010"),
        Token::Integer(0b10101010)
    ));
}

// ---- Decimal with underscores ----

#[test]
fn decimal_underscore_integer() {
    assert!(matches!(lex_single("1_000_000"), Token::Integer(1_000_000)));
}

#[test]
fn decimal_underscore_float() {
    assert!(matches!(lex_single("1_000.5_5"), Token::Float(f) if (f - 1000.55).abs() < 1e-9));
}

// ---- Scientific notation (bug fix) ----

#[test]
fn scientific_with_dot() {
    assert!(matches!(lex_single("1.5e10"), Token::Float(f) if (f - 1.5e10).abs() < 1.0));
}

#[test]
fn scientific_without_dot() {
    assert!(matches!(lex_single("1e10"), Token::Float(f) if (f - 1e10).abs() < 1.0));
}

#[test]
fn scientific_negative_exponent() {
    assert!(matches!(lex_single("2.3e-5"), Token::Float(f) if (f - 2.3e-5).abs() < 1e-15));
}

#[test]
fn scientific_positive_exponent() {
    assert!(matches!(lex_single("1e+10"), Token::Float(f) if (f - 1e10).abs() < 1.0));
}

#[test]
fn scientific_uppercase_e() {
    assert!(matches!(lex_single("1.5E10"), Token::Float(f) if (f - 1.5e10).abs() < 1.0));
}

#[test]
fn scientific_underscore_in_exponent() {
    assert!(matches!(lex_single("1.5e1_0"), Token::Float(f) if (f - 1.5e10).abs() < 1.0));
}

#[test]
fn scientific_positive_sign() {
    assert!(matches!(lex_single("+1.5e10"), Token::Float(f) if (f - 1.5e10).abs() < 1.0));
}

// ---- Backward compatibility ----

#[test]
fn decimal_plain_integer() {
    assert!(matches!(lex_single("42"), Token::Integer(42)));
}

#[test]
fn decimal_plain_float() {
    assert!(matches!(lex_single("2.71"), Token::Float(f) if (f - 2.71_f64).abs() < 1e-9));
}

#[test]
fn decimal_negative_integer() {
    assert!(matches!(lex_single("-42"), Token::Integer(-42)));
}

#[test]
fn decimal_zero() {
    assert!(matches!(lex_single("0"), Token::Integer(0)));
}

#[test]
fn decimal_leading_zero_stays_decimal() {
    // 042 is decimal 42, not octal
    assert!(matches!(lex_single("042"), Token::Integer(42)));
}

// ---- Error cases ----

#[test]
fn hex_invalid_digit_error() {
    let e = lex_err("0xGG");
    assert!(e.contains("Invalid hexadecimal integer"), "got: {e}");
}

#[test]
fn hex_empty_body_error() {
    let e = lex_err("0x");
    assert!(e.contains("Invalid hexadecimal integer"), "got: {e}");
}

#[test]
fn octal_invalid_digit_error() {
    let e = lex_err("0o888");
    assert!(e.contains("Invalid octal integer"), "got: {e}");
}

#[test]
fn octal_empty_body_error() {
    let e = lex_err("0o");
    assert!(e.contains("Invalid octal integer"), "got: {e}");
}

#[test]
fn binary_invalid_digit_error() {
    let e = lex_err("0b123");
    assert!(e.contains("Invalid binary integer"), "got: {e}");
}

#[test]
fn binary_empty_body_error() {
    let e = lex_err("0b");
    assert!(e.contains("Invalid binary integer"), "got: {e}");
}

#[test]
fn underscore_consecutive_error() {
    let e = lex_err("1__000");
    assert!(e.contains("Invalid underscore"), "got: {e}");
}

#[test]
fn underscore_trailing_error() {
    let e = lex_err("1_");
    assert!(e.contains("Invalid underscore"), "got: {e}");
}

#[test]
fn underscore_leading_after_hex_prefix_error() {
    let e = lex_err("0x_FF");
    assert!(e.contains("Invalid underscore"), "got: {e}");
}

#[test]
fn underscore_before_dot_error() {
    let e = lex_err("1_.5");
    assert!(e.contains("Invalid underscore"), "got: {e}");
}

#[test]
fn underscore_after_dot_error() {
    let e = lex_err("1._5");
    assert!(e.contains("Invalid underscore"), "got: {e}");
}

#[test]
fn underscore_before_exponent_marker_error() {
    let e = lex_err("1_e10");
    assert!(e.contains("Invalid underscore"), "got: {e}");
}

#[test]
fn underscore_after_exponent_marker_error() {
    let e = lex_err("1e_10");
    assert!(e.contains("Invalid underscore"), "got: {e}");
}

#[test]
fn scientific_missing_exponent_digits_error() {
    let e = lex_err("1.5e");
    assert!(e.contains("Invalid float"), "got: {e}");
}

#[test]
fn scientific_sign_no_exponent_digits_error_pos() {
    let e = lex_err("1.5e+");
    assert!(e.contains("Invalid float"), "got: {e}");
}

#[test]
fn scientific_sign_no_exponent_digits_error_neg() {
    let e = lex_err("1.5e-");
    assert!(e.contains("Invalid float"), "got: {e}");
}
