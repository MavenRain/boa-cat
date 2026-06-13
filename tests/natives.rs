//! Integration tests for the native-callable embedding path.

// Native callable signatures are fixed by `NativeFn`; clippy's pass-by-value
// and unnecessary-wraps lints fire spuriously on conforming bodies.
#![allow(clippy::needless_pass_by_value)]
#![allow(clippy::unnecessary_wraps)]

use boa_cat::env::Env;
use boa_cat::fuel::Fuel;
use boa_cat::heap::Heap;
use boa_cat::outcome::{EvalResult, Outcome};
use boa_cat::value::Object;
use boa_cat::{Cell, Error, NativeFn, Value, evaluate_program_with};
use ecma_lex_cat::lex;
use ecma_parse_cat::parse_script;

fn add_one(args: Vec<Value>, _this: Value, heap: Heap, fuel: Fuel) -> EvalResult {
    let n = match args.first() {
        Some(Value::Number(n)) => *n,
        _other => 0.0,
    };
    Ok((Outcome::Normal(Value::Number(n + 1.0)), heap, fuel))
}

fn make_pair(args: Vec<Value>, _this: Value, heap: Heap, fuel: Fuel) -> EvalResult {
    let a = args.first().cloned().unwrap_or(Value::Undefined);
    let b = args.get(1).cloned().unwrap_or(Value::Undefined);
    let mut props = std::collections::BTreeMap::new();
    let _ = props.insert("first".to_owned(), a);
    let _ = props.insert("second".to_owned(), b);
    let (id, heap) = heap.alloc_object(Object::from_properties(props));
    Ok((Outcome::Normal(Value::Object(id)), heap, fuel))
}

fn run_with_natives(source: &str, bindings: Vec<(&str, NativeFn)>) -> Result<Value, Error> {
    let tokens = lex(source)?;
    let program = parse_script(&tokens)?;
    let heap = Heap::new();
    let (env, heap) = bindings
        .into_iter()
        .fold((Env::empty(), heap), |(env, heap), (name, f)| {
            let (cell_id, heap) = heap.alloc_cell(Cell::new(Value::Native(f), false));
            (env.extend_cell(name, cell_id), heap)
        });
    evaluate_program_with(&program, env, heap, Fuel::new(10_000)).map(|(v, _)| v)
}

#[test]
fn calls_native_returning_number() -> Result<(), Error> {
    let value = run_with_natives("addOne(41)", vec![("addOne", add_one)])?;
    assert!(
        matches!(value, Value::Number(n) if (n - 42.0).abs() < 1e-9),
        "got {value:?}"
    );
    Ok(())
}

#[test]
fn calls_native_returning_object() -> Result<(), Error> {
    let tokens = lex("const p = pair(1, 2); p.first + p.second")?;
    let program = parse_script(&tokens)?;
    let heap = Heap::new();
    let (cell_id, heap) = heap.alloc_cell(Cell::new(Value::Native(make_pair), false));
    let env = Env::empty().extend_cell("pair", cell_id);
    let (value, _) = evaluate_program_with(&program, env, heap, Fuel::new(10_000))?;
    assert!(
        matches!(value, Value::Number(n) if (n - 3.0).abs() < 1e-9),
        "got {value:?}"
    );
    Ok(())
}

#[test]
fn new_on_native_returning_object_uses_returned_object() -> Result<(), Error> {
    let value = run_with_natives(
        "const p = new pair(1, 2); p.first + p.second",
        vec![("pair", make_pair)],
    )?;
    assert!(
        matches!(value, Value::Number(n) if (n - 3.0).abs() < 1e-9),
        "got {value:?}"
    );
    Ok(())
}

#[test]
fn new_on_native_returning_non_object_falls_back_to_this() -> Result<(), Error> {
    // `addOne` returns Value::Number; per the spec, `new` on a
    // constructor that returns a primitive should ignore the
    // returned value and yield `this` (which is the freshly
    // allocated empty Object).  `typeof` confirms this is an
    // Object, not the Number addOne returned.
    let value = run_with_natives("typeof (new addOne(41))", vec![("addOne", add_one)])?;
    assert!(
        matches!(value, Value::String(ref s) if s == "object"),
        "got {value:?}"
    );
    Ok(())
}

#[test]
fn new_on_native_passes_args_to_native_fn() -> Result<(), Error> {
    // `pair(a, b)` reads args[0] / args[1]; `new pair(1, 2)`
    // should land them in the constructor body the same way.
    let value = run_with_natives(
        "(new pair('alpha', 'beta')).first",
        vec![("pair", make_pair)],
    )?;
    assert!(
        matches!(value, Value::String(ref s) if s == "alpha"),
        "got {value:?}"
    );
    Ok(())
}
