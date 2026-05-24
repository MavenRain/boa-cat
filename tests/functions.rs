//! Integration tests covering functions, closures, and returns.

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
fn function_declaration() -> Result<(), Error> {
    assert_number(
        &eval("function add(a, b) { return a + b; } add(3, 4)")?,
        7.0,
    )
}

#[test]
fn function_expression() -> Result<(), Error> {
    assert_number(
        &eval("const add = function(a, b) { return a + b; }; add(2, 5)")?,
        7.0,
    )
}

#[test]
fn arrow_function_concise() -> Result<(), Error> {
    assert_number(&eval("const double = x => x * 2; double(21)")?, 42.0)
}

#[test]
fn arrow_function_block() -> Result<(), Error> {
    assert_number(
        &eval("const add = (a, b) => { return a + b; }; add(10, 20)")?,
        30.0,
    )
}

#[test]
fn closure_captures_outer() -> Result<(), Error> {
    assert_number(
        &eval(
            "function makeCounter() {
                let n = 0;
                return function() { n += 1; return n; };
            }
            const c = makeCounter();
            c(); c(); c()",
        )?,
        3.0,
    )
}

#[test]
fn recursion() -> Result<(), Error> {
    assert_number(
        &eval(
            "function fact(n) {
                if (n <= 1) { return 1; }
                return n * fact(n - 1);
            }
            fact(5)",
        )?,
        120.0,
    )
}

#[test]
fn higher_order_map_pattern() -> Result<(), Error> {
    assert_number(
        &eval(
            "function apply(f, x) { return f(x); }
            const inc = x => x + 1;
            apply(inc, 41)",
        )?,
        42.0,
    )
}

#[test]
fn template_literal() -> Result<(), Error> {
    assert_string(
        &eval("const name = \"world\"; `hello, ${name}!`")?,
        "hello, world!",
    )
}
