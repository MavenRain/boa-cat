//! Integration tests covering arithmetic, coercion, and basic values.

use boa_cat::{Error, Value, run};

fn eval(source: &str) -> Result<Value, Error> {
    run(source).run()
}

fn assert_number(actual: &Value, expected: f64) -> Result<(), Error> {
    matches!(actual, Value::Number(n) if (*n - expected).abs() < 1e-9)
        .then_some(())
        .ok_or(Error::UncaughtException {
            rendered: format!("expected Number({expected}), got {actual:?}"),
        })
}

fn assert_string(actual: &Value, expected: &str) -> Result<(), Error> {
    matches!(actual, Value::String(s) if s == expected)
        .then_some(())
        .ok_or(Error::UncaughtException {
            rendered: format!("expected String({expected:?}), got {actual:?}"),
        })
}

fn assert_boolean(actual: &Value, expected: bool) -> Result<(), Error> {
    matches!(actual, Value::Boolean(b) if *b == expected)
        .then_some(())
        .ok_or(Error::UncaughtException {
            rendered: format!("expected Boolean({expected}), got {actual:?}"),
        })
}

#[test]
fn integer_literal() -> Result<(), Error> {
    assert_number(&eval("42")?, 42.0)
}

#[test]
fn arithmetic_precedence() -> Result<(), Error> {
    assert_number(&eval("1 + 2 * 3")?, 7.0)
}

#[test]
fn parens_override_precedence() -> Result<(), Error> {
    assert_number(&eval("(1 + 2) * 3")?, 9.0)
}

#[test]
fn exponentiation_right_associative() -> Result<(), Error> {
    assert_number(&eval("2 ** 3 ** 2")?, 512.0)
}

#[test]
fn string_concatenation() -> Result<(), Error> {
    assert_string(&eval("\"hello\" + \" \" + \"world\"")?, "hello world")
}

#[test]
fn number_string_concatenation() -> Result<(), Error> {
    assert_string(&eval("\"x = \" + (1 + 2)")?, "x = 3")
}

#[test]
fn strict_equality() -> Result<(), Error> {
    assert_boolean(&eval("1 === 1")?, true)?;
    assert_boolean(&eval("1 === \"1\"")?, false)
}

#[test]
fn loose_equality_string_number() -> Result<(), Error> {
    assert_boolean(&eval("1 == \"1\"")?, true)
}

#[test]
fn unary_minus() -> Result<(), Error> {
    assert_number(&eval("-5")?, -5.0)
}

#[test]
fn typeof_operator() -> Result<(), Error> {
    assert_string(&eval("typeof 42")?, "number")?;
    assert_string(&eval("typeof \"x\"")?, "string")?;
    assert_string(&eval("typeof true")?, "boolean")?;
    assert_string(&eval("typeof undefined")?, "undefined")?;
    assert_string(&eval("typeof null")?, "object")
}

#[test]
fn logical_short_circuit() -> Result<(), Error> {
    assert_number(&eval("1 || 2")?, 1.0)?;
    assert_number(&eval("0 || 2")?, 2.0)?;
    assert_number(&eval("1 && 2")?, 2.0)?;
    assert_number(&eval("0 && 2")?, 0.0)
}

#[test]
fn nullish_coalescing() -> Result<(), Error> {
    assert_number(&eval("null ?? 5")?, 5.0)?;
    assert_number(&eval("0 ?? 5")?, 0.0)
}

#[test]
fn conditional_expression() -> Result<(), Error> {
    assert_number(&eval("true ? 1 : 2")?, 1.0)?;
    assert_number(&eval("false ? 1 : 2")?, 2.0)
}

#[test]
fn bitwise_or() -> Result<(), Error> {
    assert_number(&eval("5 | 3")?, 7.0)
}

#[test]
fn left_shift() -> Result<(), Error> {
    assert_number(&eval("1 << 4")?, 16.0)
}
