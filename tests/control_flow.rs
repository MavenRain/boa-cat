//! Integration tests covering control-flow statements.

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

#[test]
fn variable_declaration() -> Result<(), Error> {
    assert_number(&eval("let x = 5; x")?, 5.0)
}

#[test]
fn assignment() -> Result<(), Error> {
    assert_number(&eval("let x = 5; x = 10; x")?, 10.0)
}

#[test]
fn compound_assignment() -> Result<(), Error> {
    assert_number(&eval("let x = 5; x += 3; x")?, 8.0)
}

#[test]
fn const_declaration() -> Result<(), Error> {
    assert_number(&eval("const x = 42; x")?, 42.0)
}

#[test]
fn if_statement() -> Result<(), Error> {
    assert_string(
        &eval("let r = \"\"; if (1 < 2) { r = \"yes\"; } else { r = \"no\"; } r")?,
        "yes",
    )
}

#[test]
fn while_loop() -> Result<(), Error> {
    assert_number(
        &eval("let i = 0; let s = 0; while (i < 10) { s += i; i += 1; } s")?,
        45.0,
    )
}

#[test]
fn for_loop() -> Result<(), Error> {
    assert_number(
        &eval("let s = 0; for (let i = 0; i < 10; i = i + 1) { s += i; } s")?,
        45.0,
    )
}

#[test]
fn do_while_loop() -> Result<(), Error> {
    assert_number(&eval("let i = 0; do { i += 1; } while (i < 5); i")?, 5.0)
}

#[test]
fn break_in_while() -> Result<(), Error> {
    assert_number(
        &eval("let i = 0; while (i < 100) { if (i === 5) { break; } i += 1; } i")?,
        5.0,
    )
}

#[test]
fn continue_in_while() -> Result<(), Error> {
    assert_number(
        &eval(
            "let i = 0; let evens = 0; while (i < 10) { i += 1; if (i % 2 === 1) { continue; } evens += 1; } evens",
        )?,
        5.0,
    )
}
