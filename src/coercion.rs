//! ECMA-262 abstract operations for value coercion.
//!
//! Implements `ToBoolean`, `ToNumber`, `ToString`, `ToPropertyKey`, and
//! `ToInt32`/`ToUint32` used by binary operators and member access.

use crate::heap::Heap;
use crate::value::Value;

/// `ToBoolean(value)`.
#[must_use]
pub fn to_boolean(value: &Value) -> bool {
    match value {
        Value::Undefined | Value::Null => false,
        Value::Boolean(b) => *b,
        Value::Number(n) => !(n.is_nan() || *n == 0.0),
        Value::String(s) => !s.is_empty(),
        Value::Object(_) | Value::Function(_) => true,
    }
}

/// `ToNumber(value)`.
#[must_use]
#[allow(clippy::match_same_arms)] // ECMA-262 ToNumber: Undefined and Object/Function both yield NaN by different spec paths
pub fn to_number(value: &Value) -> f64 {
    match value {
        Value::Undefined => f64::NAN,
        Value::Null | Value::Boolean(false) => 0.0,
        Value::Boolean(true) => 1.0,
        Value::Number(n) => *n,
        Value::String(s) => string_to_number(s),
        Value::Object(_) | Value::Function(_) => f64::NAN,
    }
}

fn string_to_number(s: &str) -> f64 {
    let trimmed = s.trim();
    if trimmed.is_empty() {
        0.0
    } else {
        trimmed.parse::<f64>().unwrap_or(f64::NAN)
    }
}

/// `ToString(value)`.  For objects v0 yields a placeholder repr; richer
/// `[[ToPrimitive]]` is deferred until `ecma-runtime-cat` provides
/// `toString` lookups.
#[must_use]
pub fn to_string(value: &Value, _heap: &Heap) -> String {
    match value {
        Value::Undefined => "undefined".to_owned(),
        Value::Null => "null".to_owned(),
        Value::Boolean(true) => "true".to_owned(),
        Value::Boolean(false) => "false".to_owned(),
        Value::Number(n) => number_to_string(*n),
        Value::String(s) => s.clone(),
        Value::Object(id) => format!("[object Object#{}]", id.raw()),
        Value::Function(id) => format!("function fn#{}() {{ [native code] }}", id.raw()),
    }
}

fn number_to_string(n: f64) -> String {
    if n.is_nan() {
        "NaN".to_owned()
    } else if n.is_infinite() {
        if n.is_sign_negative() {
            "-Infinity".to_owned()
        } else {
            "Infinity".to_owned()
        }
    } else if n == 0.0 {
        "0".to_owned()
    } else {
        format!("{n}")
    }
}

/// `ToPropertyKey(value)`: only the string form for v0 (Symbol keys
/// deferred).  Numeric keys are normalised to their canonical string repr.
#[must_use]
pub fn to_property_key(value: &Value, heap: &Heap) -> String {
    to_string(value, heap)
}

/// `ToInt32(value)` per ECMA-262.
#[must_use]
pub fn to_int32(value: &Value) -> i32 {
    let n = to_number(value);
    if n.is_finite() {
        let truncated = n.trunc();
        let modulo = euclid_mod(truncated, 4_294_967_296.0);
        if modulo >= 2_147_483_648.0 {
            (modulo - 4_294_967_296.0) as i32
        } else {
            modulo as i32
        }
    } else {
        0
    }
}

/// `ToUint32(value)` per ECMA-262.
#[must_use]
pub fn to_uint32(value: &Value) -> u32 {
    let n = to_number(value);
    if n.is_finite() {
        let truncated = n.trunc();
        euclid_mod(truncated, 4_294_967_296.0) as u32
    } else {
        0
    }
}

fn euclid_mod(value: f64, modulus: f64) -> f64 {
    let r = value % modulus;
    if r < 0.0 { r + modulus } else { r }
}

/// `SameValueZero(a, b)`: equality with NaN-equals-NaN and `+0 === -0`.
#[must_use]
pub fn same_value_zero(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => {
            if x.is_nan() && y.is_nan() {
                true
            } else {
                x == y
            }
        }
        _other => strict_equals_non_numeric(a, b),
    }
}

/// `IsStrictlyEqual(a, b)` (`===`).
#[must_use]
pub fn strict_equals(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => {
            if x.is_nan() || y.is_nan() {
                false
            } else {
                x == y
            }
        }
        _other => strict_equals_non_numeric(a, b),
    }
}

fn strict_equals_non_numeric(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Undefined, Value::Undefined) | (Value::Null, Value::Null) => true,
        (Value::Boolean(x), Value::Boolean(y)) => x == y,
        (Value::String(x), Value::String(y)) => x == y,
        (Value::Object(x), Value::Object(y)) => x == y,
        (Value::Function(x), Value::Function(y)) => x == y,
        _other => false,
    }
}

/// `IsLooselyEqual(a, b)` (`==`).  Implements the value-coercion table.
#[must_use]
#[allow(clippy::match_same_arms)] // Undefined-Null and same-type-pairs are different spec paths grouped for readability
pub fn loose_equals(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Undefined, Value::Null) | (Value::Null, Value::Undefined) => true,
        (Value::Undefined, Value::Undefined) | (Value::Null, Value::Null) => true,
        (Value::Number(x), Value::Number(y)) => !x.is_nan() && !y.is_nan() && x == y,
        (Value::String(x), Value::String(y)) => x == y,
        (Value::Boolean(_), Value::Boolean(_))
        | (Value::Object(_), Value::Object(_))
        | (Value::Function(_), Value::Function(_)) => strict_equals(a, b),
        (Value::Number(x), Value::String(_)) => {
            let n = to_number(b);
            !n.is_nan() && !x.is_nan() && *x == n
        }
        (Value::String(_), Value::Number(_)) => loose_equals(b, a),
        (Value::Boolean(_), _other) => loose_equals(&Value::Number(to_number(a)), b),
        (_other, Value::Boolean(_)) => loose_equals(a, &Value::Number(to_number(b))),
        _other => false,
    }
}
