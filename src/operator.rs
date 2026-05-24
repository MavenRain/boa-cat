//! Binary, unary, and logical operator semantics per ECMA-262.

use ecma_syntax_cat::operator::{BinaryOperator, UnaryOperator};

use crate::coercion::{
    loose_equals, strict_equals, to_boolean, to_int32, to_number, to_string, to_uint32,
};
use crate::error::Error;
use crate::heap::Heap;
use crate::value::Value;

/// Apply a binary operator to two values.
///
/// # Errors
///
/// [`Error::Unsupported`] for operators v0 does not implement
/// (`in`, `instanceof` require live object inspection).
pub fn apply_binary(
    operator: BinaryOperator,
    lhs: &Value,
    rhs: &Value,
    heap: &Heap,
) -> Result<Value, Error> {
    match operator {
        BinaryOperator::Equal => Ok(Value::Boolean(loose_equals(lhs, rhs))),
        BinaryOperator::NotEqual => Ok(Value::Boolean(!loose_equals(lhs, rhs))),
        BinaryOperator::StrictEqual => Ok(Value::Boolean(strict_equals(lhs, rhs))),
        BinaryOperator::StrictNotEqual => Ok(Value::Boolean(!strict_equals(lhs, rhs))),
        BinaryOperator::LessThan => Ok(Value::Boolean(less_than(lhs, rhs, heap))),
        BinaryOperator::LessThanOrEqual => Ok(Value::Boolean(!less_than(rhs, lhs, heap))),
        BinaryOperator::GreaterThan => Ok(Value::Boolean(less_than(rhs, lhs, heap))),
        BinaryOperator::GreaterThanOrEqual => Ok(Value::Boolean(!less_than(lhs, rhs, heap))),
        BinaryOperator::LeftShift => Ok(shift_left(lhs, rhs)),
        BinaryOperator::RightShift => Ok(shift_right(lhs, rhs)),
        BinaryOperator::UnsignedRightShift => Ok(unsigned_shift_right(lhs, rhs)),
        BinaryOperator::Add => Ok(add(lhs, rhs, heap)),
        BinaryOperator::Subtract => Ok(Value::Number(to_number(lhs) - to_number(rhs))),
        BinaryOperator::Multiply => Ok(Value::Number(to_number(lhs) * to_number(rhs))),
        BinaryOperator::Divide => Ok(Value::Number(to_number(lhs) / to_number(rhs))),
        BinaryOperator::Remainder => Ok(Value::Number(to_number(lhs) % to_number(rhs))),
        BinaryOperator::Exponentiation => Ok(Value::Number(to_number(lhs).powf(to_number(rhs)))),
        BinaryOperator::BitwiseOr => Ok(bitwise(lhs, rhs, |a, b| a | b)),
        BinaryOperator::BitwiseXor => Ok(bitwise(lhs, rhs, |a, b| a ^ b)),
        BinaryOperator::BitwiseAnd => Ok(bitwise(lhs, rhs, |a, b| a & b)),
        BinaryOperator::In | BinaryOperator::InstanceOf => Err(Error::Unsupported {
            feature: "`in` / `instanceof` operators",
        }),
    }
}

fn add(lhs: &Value, rhs: &Value, heap: &Heap) -> Value {
    match (lhs, rhs) {
        (Value::String(a), b) => Value::String(format!("{a}{}", to_string(b, heap))),
        (a, Value::String(b)) => Value::String(format!("{}{b}", to_string(a, heap))),
        _other => Value::Number(to_number(lhs) + to_number(rhs)),
    }
}

fn less_than(lhs: &Value, rhs: &Value, _heap: &Heap) -> bool {
    match (lhs, rhs) {
        (Value::String(a), Value::String(b)) => a < b,
        _other => {
            let a = to_number(lhs);
            let b = to_number(rhs);
            !a.is_nan() && !b.is_nan() && a < b
        }
    }
}

#[allow(clippy::cast_sign_loss, clippy::cast_possible_wrap)]
fn shift_left(lhs: &Value, rhs: &Value) -> Value {
    let l = to_int32(lhs);
    let shift = to_uint32(rhs) & 0x1f;
    Value::Number(f64::from(l.wrapping_shl(shift)))
}

#[allow(clippy::cast_sign_loss, clippy::cast_possible_wrap)]
fn shift_right(lhs: &Value, rhs: &Value) -> Value {
    let l = to_int32(lhs);
    let shift = to_uint32(rhs) & 0x1f;
    Value::Number(f64::from(l.wrapping_shr(shift)))
}

#[allow(clippy::cast_sign_loss, clippy::cast_possible_wrap)]
fn unsigned_shift_right(lhs: &Value, rhs: &Value) -> Value {
    let l = to_uint32(lhs);
    let shift = to_uint32(rhs) & 0x1f;
    Value::Number(f64::from(l.wrapping_shr(shift)))
}

fn bitwise(lhs: &Value, rhs: &Value, combine: impl Fn(i32, i32) -> i32) -> Value {
    let l = to_int32(lhs);
    let r = to_int32(rhs);
    Value::Number(f64::from(combine(l, r)))
}

/// Apply a unary operator to a value.
///
/// `delete` is not represented as a pure unary -- it requires the original
/// member expression -- so this function only handles the pure-value
/// operators.  `Delete` is rejected with [`Error::Unsupported`].
///
/// # Errors
///
/// [`Error::Unsupported`] for `delete` (handled at the expression level).
pub fn apply_unary(operator: UnaryOperator, operand: &Value, heap: &Heap) -> Result<Value, Error> {
    match operator {
        UnaryOperator::Minus => Ok(Value::Number(-to_number(operand))),
        UnaryOperator::Plus => Ok(Value::Number(to_number(operand))),
        UnaryOperator::LogicalNot => Ok(Value::Boolean(!to_boolean(operand))),
        UnaryOperator::BitwiseNot => Ok(Value::Number(f64::from(!to_int32(operand)))),
        UnaryOperator::TypeOf => Ok(Value::String(type_of(operand))),
        UnaryOperator::Void => Ok(Value::Undefined),
        UnaryOperator::Delete => Err(Error::Unsupported {
            feature: "`delete` operator (handled at expression level)",
        }),
    }
    .map(|v| match v {
        Value::Undefined => {
            let _ = heap;
            Value::Undefined
        }
        other => other,
    })
}

#[must_use]
#[allow(clippy::match_same_arms)] // Null and Object both yield "object" per ECMA-262 typeof historical bug
fn type_of(value: &Value) -> String {
    match value {
        Value::Undefined => "undefined",
        Value::Null => "object",
        Value::Boolean(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Object(_) => "object",
        Value::Function(_) => "function",
    }
    .to_owned()
}
