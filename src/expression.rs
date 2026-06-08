//! Expression evaluation.

use std::collections::BTreeMap;

use ecma_syntax_cat::expression::{
    Expression, ExpressionKind, MemberProperty, ObjectMember, ObjectPropertyKind, PropertyKey,
};
use ecma_syntax_cat::literal::Literal;
use ecma_syntax_cat::operator::{
    AssignmentOperator, BinaryOperator, UnaryOperator, UpdateOperator,
};
use ecma_syntax_cat::pattern::Pattern;

use crate::coercion::{to_boolean, to_number, to_property_key, to_string, to_uint32};
use crate::env::{Binding, Env};
use crate::error::Error;
use crate::fuel::Fuel;
use crate::heap::Heap;
use crate::operator::{apply_binary, apply_unary};
use crate::outcome::{EvalResult, Outcome, step};
use crate::value::{Cell, FunctionBody, FunctionDef, Object, Value};

/// Evaluate `expr` in `env`, `heap`, and `fuel`.
///
/// # Errors
///
/// Propagates fatal [`Error`] conditions (out-of-fuel, unsupported AST,
/// parser errors).  Recoverable exceptions surface as [`Outcome::Throw`].
pub fn eval(expr: &Expression, env: &Env, heap: Heap, fuel: Fuel) -> EvalResult {
    let fuel = fuel.spend()?;
    match expr.value() {
        ExpressionKind::This => eval_this(env, heap, fuel),
        ExpressionKind::Identifier(id) => eval_identifier(id.as_str(), env, heap, fuel),
        ExpressionKind::Literal(lit) => eval_literal(lit, heap, fuel),
        ExpressionKind::Template {
            quasis,
            expressions,
        } => eval_template(quasis, expressions, env, heap, fuel),
        ExpressionKind::Array { elements } => eval_array(elements, env, heap, fuel),
        ExpressionKind::Object { properties } => eval_object(properties, env, heap, fuel),
        ExpressionKind::Member {
            object,
            property,
            optional,
        } => eval_member(object, property, *optional, env, heap, fuel),
        ExpressionKind::Call {
            callee,
            arguments,
            optional,
        } => eval_call(callee, arguments, *optional, env, heap, fuel),
        ExpressionKind::New { callee, arguments } => eval_new(callee, arguments, env, heap, fuel),
        ExpressionKind::Update {
            operator,
            argument,
            prefix,
        } => eval_update(*operator, argument, *prefix, env, heap, fuel),
        ExpressionKind::Unary { operator, argument } => {
            eval_unary(*operator, argument, env, heap, fuel)
        }
        ExpressionKind::Binary {
            operator,
            left,
            right,
        } => eval_binary(*operator, left, right, env, heap, fuel),
        ExpressionKind::Logical {
            operator,
            left,
            right,
        } => eval_logical(*operator, left, right, env, heap, fuel),
        ExpressionKind::Conditional {
            test,
            consequent,
            alternate,
        } => eval_conditional(test, consequent, alternate, env, heap, fuel),
        ExpressionKind::Assignment {
            operator,
            left,
            right,
        } => eval_assignment(*operator, left, right, env, heap, fuel),
        ExpressionKind::Sequence { expressions } => eval_sequence(expressions, env, heap, fuel),
        ExpressionKind::Spread { .. } => Err(Error::Unsupported {
            feature: "bare spread outside array/call",
        }),
        ExpressionKind::ArrowFunction(arrow) => eval_arrow_function(arrow, env, heap, fuel),
        ExpressionKind::FunctionExpression(func) => eval_function_expression(func, env, heap, fuel),
        ExpressionKind::Chain { expression } | ExpressionKind::Parenthesized { expression } => {
            eval(expression, env, heap, fuel)
        }
        ExpressionKind::ClassExpression(_) => Err(Error::Unsupported {
            feature: "class expression",
        }),
        ExpressionKind::Yield { .. } => Err(Error::Unsupported { feature: "yield" }),
        ExpressionKind::Await { .. } => Err(Error::Unsupported { feature: "await" }),
        ExpressionKind::TaggedTemplate { .. } => Err(Error::Unsupported {
            feature: "tagged template",
        }),
        ExpressionKind::ImportExpression { .. } => Err(Error::Unsupported {
            feature: "import() expression",
        }),
        ExpressionKind::MetaProperty { .. } => Err(Error::Unsupported {
            feature: "meta property",
        }),
        ExpressionKind::PrivateIdentifier(_) => Err(Error::Unsupported {
            feature: "private identifier in expression position",
        }),
        ExpressionKind::Super => Err(Error::Unsupported { feature: "super" }),
    }
}

#[allow(clippy::unnecessary_wraps)] // uniform EvalResult signature lets eval() call sites use `?`
fn eval_this(env: &Env, heap: Heap, fuel: Fuel) -> EvalResult {
    let value = match env.lookup("__this__") {
        Some(Binding::Direct(v)) => v.clone(),
        Some(Binding::Cell(id)) => heap
            .cell(*id)
            .map_or(Value::Undefined, |c| c.value().clone()),
        None => Value::Undefined,
    };
    Ok((Outcome::Normal(value), heap, fuel))
}

#[allow(clippy::unnecessary_wraps)] // uniform EvalResult signature
fn eval_identifier(name: &str, env: &Env, heap: Heap, fuel: Fuel) -> EvalResult {
    let outcome = match env.lookup(name) {
        Some(Binding::Direct(v)) => Outcome::Normal(v.clone()),
        Some(Binding::Cell(id)) => heap.cell(*id).map_or_else(
            || Outcome::Throw(reference_error(name)),
            |c| Outcome::Normal(c.value().clone()),
        ),
        None => Outcome::Throw(reference_error(name)),
    };
    Ok((outcome, heap, fuel))
}

#[allow(clippy::unnecessary_wraps)] // uniform EvalResult signature
#[allow(clippy::match_same_arms)] // BigInt and RegExp share a fallback path but the error text differs
fn eval_literal(lit: &Literal, heap: Heap, fuel: Fuel) -> EvalResult {
    let outcome = match lit {
        Literal::Number(n) => Outcome::Normal(Value::Number(*n)),
        Literal::String(s) => Outcome::Normal(Value::String(s.clone())),
        Literal::Boolean(b) => Outcome::Normal(Value::Boolean(*b)),
        Literal::Null => Outcome::Normal(Value::Null),
        Literal::BigInt(_) => Outcome::Throw(Value::String(
            "TypeError: BigInt evaluation not implemented".to_owned(),
        )),
        Literal::RegExp { .. } => Outcome::Throw(Value::String(
            "TypeError: RegExp evaluation not implemented".to_owned(),
        )),
    };
    Ok((outcome, heap, fuel))
}

fn eval_template(
    quasis: &[String],
    expressions: &[Expression],
    env: &Env,
    heap: Heap,
    fuel: Fuel,
) -> EvalResult {
    eval_template_recursive(quasis, expressions, 0, String::new(), env, heap, fuel)
}

fn eval_template_recursive(
    quasis: &[String],
    expressions: &[Expression],
    idx: usize,
    acc: String,
    env: &Env,
    heap: Heap,
    fuel: Fuel,
) -> EvalResult {
    let quasi = quasis.get(idx).cloned().unwrap_or_default();
    let extended = format!("{acc}{quasi}");
    expressions.get(idx).map_or_else(
        || {
            Ok((
                Outcome::Normal(Value::String(extended.clone())),
                heap.clone(),
                fuel,
            ))
        },
        |expr| {
            step(eval(expr, env, heap.clone(), fuel), |v, heap, fuel| {
                let next = format!("{extended}{}", to_string(&v, &heap));
                eval_template_recursive(quasis, expressions, idx + 1, next, env, heap, fuel)
            })
        },
    )
}

fn eval_array(elements: &[Option<Expression>], env: &Env, heap: Heap, fuel: Fuel) -> EvalResult {
    collect_array_values(elements, 0, Vec::new(), env, heap, fuel).map(|(outcome, heap, fuel)| {
        match outcome {
            ArrayOutcome::Throw(v) => (Outcome::Throw(v), heap, fuel),
            ArrayOutcome::Values(values) => {
                let object = build_array_object(&values);
                let (id, heap) = heap.alloc_object(object);
                (Outcome::Normal(Value::Object(id)), heap, fuel)
            }
        }
    })
}

enum ArrayOutcome {
    Values(Vec<Value>),
    Throw(Value),
}

fn collect_array_values(
    elements: &[Option<Expression>],
    idx: usize,
    acc: Vec<Value>,
    env: &Env,
    heap: Heap,
    fuel: Fuel,
) -> Result<(ArrayOutcome, Heap, Fuel), Error> {
    elements.get(idx).map_or_else(
        || Ok((ArrayOutcome::Values(acc.clone()), heap.clone(), fuel)),
        |slot| {
            slot.as_ref().map_or_else(
                || {
                    let extended = append_value(acc.clone(), Value::Undefined);
                    collect_array_values(elements, idx + 1, extended, env, heap.clone(), fuel)
                },
                |expr| match expr.value() {
                    ExpressionKind::Spread { argument } => eval(argument, env, heap.clone(), fuel)
                        .and_then(|(out, heap, fuel)| match out {
                            Outcome::Throw(v) => Ok((ArrayOutcome::Throw(v), heap, fuel)),
                            Outcome::Normal(spread_val) => {
                                let spread_items = collect_spread_items(&spread_val, &heap);
                                let extended: Vec<Value> =
                                    acc.clone().into_iter().chain(spread_items).collect();
                                collect_array_values(elements, idx + 1, extended, env, heap, fuel)
                            }
                        }),
                    _other => {
                        eval(expr, env, heap.clone(), fuel).and_then(
                            |(out, heap, fuel)| match out {
                                Outcome::Throw(v) => Ok((ArrayOutcome::Throw(v), heap, fuel)),
                                Outcome::Normal(v) => {
                                    let extended = append_value(acc.clone(), v);
                                    collect_array_values(
                                        elements,
                                        idx + 1,
                                        extended,
                                        env,
                                        heap,
                                        fuel,
                                    )
                                }
                            },
                        )
                    }
                },
            )
        },
    )
}

fn append_value(acc: Vec<Value>, value: Value) -> Vec<Value> {
    acc.into_iter().chain(std::iter::once(value)).collect()
}

fn collect_spread_items(value: &Value, heap: &Heap) -> Vec<Value> {
    match value {
        Value::Object(id) => heap
            .object(*id)
            .map(spread_array_object)
            .unwrap_or_default(),
        Value::String(s) => s.chars().map(|c| Value::String(c.to_string())).collect(),
        _other => Vec::new(),
    }
}

fn spread_array_object(obj: &Object) -> Vec<Value> {
    let length = obj.get("length").map_or(0, to_uint32);
    (0..length)
        .map(|i| {
            obj.get(&format!("{i}"))
                .cloned()
                .unwrap_or(Value::Undefined)
        })
        .collect()
}

fn build_array_object(values: &[Value]) -> Object {
    let length = u32::try_from(values.len()).unwrap_or(u32::MAX);
    let map: BTreeMap<String, Value> = values
        .iter()
        .enumerate()
        .map(|(i, v)| (format!("{i}"), v.clone()))
        .chain(std::iter::once((
            "length".to_owned(),
            Value::Number(f64::from(length)),
        )))
        .collect();
    Object::from_properties(map)
}

fn eval_object(properties: &[ObjectMember], env: &Env, heap: Heap, fuel: Fuel) -> EvalResult {
    collect_object_members(properties, 0, ObjectAcc::default(), env, heap, fuel).map(
        |(outcome, heap, fuel)| match outcome {
            ObjectOutcome::Throw(v) => (Outcome::Throw(v), heap, fuel),
            ObjectOutcome::Acc(acc) => {
                let (id, heap) = heap.alloc_object(acc.into_object());
                (Outcome::Normal(Value::Object(id)), heap, fuel)
            }
        },
    )
}

/// Accumulator for an object-literal's resolved members.  Data and
/// accessor entries are kept in separate maps mirroring `Object`'s
/// own storage shape; the final [`Self::into_object`] folds both
/// halves into a [`Object`].
#[derive(Clone, Default)]
struct ObjectAcc {
    data: BTreeMap<String, Value>,
    accessors: BTreeMap<String, crate::value::AccessorPair>,
}

impl ObjectAcc {
    fn with_data(&self, key: String, value: Value) -> Self {
        let mut data = self.data.clone();
        let mut accessors = self.accessors.clone();
        let _ = accessors.remove(&key);
        let _ = data.insert(key, value);
        Self { data, accessors }
    }

    fn with_get(&self, key: String, get: Value) -> Self {
        let mut data = self.data.clone();
        let mut accessors = self.accessors.clone();
        let _ = data.remove(&key);
        let updated = accessors
            .get(&key)
            .cloned()
            .unwrap_or_default()
            .with_get(get);
        let _ = accessors.insert(key, updated);
        Self { data, accessors }
    }

    fn with_set(&self, key: String, set: Value) -> Self {
        let mut data = self.data.clone();
        let mut accessors = self.accessors.clone();
        let _ = data.remove(&key);
        let updated = accessors
            .get(&key)
            .cloned()
            .unwrap_or_default()
            .with_set(set);
        let _ = accessors.insert(key, updated);
        Self { data, accessors }
    }

    fn with_spread_data(&self, extension: BTreeMap<String, Value>) -> Self {
        let merged: BTreeMap<String, Value> = self
            .data
            .clone()
            .into_iter()
            .chain(extension.into_iter().filter(|(k, _)| k != "length"))
            .collect();
        Self {
            data: merged,
            accessors: self.accessors.clone(),
        }
    }

    fn into_object(self) -> Object {
        let Self { data, accessors } = self;
        accessors
            .into_iter()
            .fold(Object::from_properties(data), |obj, (key, pair)| {
                obj.with_accessor(key, pair)
            })
    }
}

enum ObjectOutcome {
    Acc(ObjectAcc),
    Throw(Value),
}

fn collect_object_members(
    properties: &[ObjectMember],
    idx: usize,
    acc: ObjectAcc,
    env: &Env,
    heap: Heap,
    fuel: Fuel,
) -> Result<(ObjectOutcome, Heap, Fuel), Error> {
    properties.get(idx).map_or_else(
        || Ok((ObjectOutcome::Acc(acc.clone()), heap.clone(), fuel)),
        |member| {
            eval_one_object_member(member, &acc, env, heap.clone(), fuel).and_then(
                |(outcome, heap, fuel)| match outcome {
                    ObjectOutcome::Throw(v) => Ok((ObjectOutcome::Throw(v), heap, fuel)),
                    ObjectOutcome::Acc(extended) => {
                        collect_object_members(properties, idx + 1, extended, env, heap, fuel)
                    }
                },
            )
        },
    )
}

fn eval_one_object_member(
    member: &ObjectMember,
    acc: &ObjectAcc,
    env: &Env,
    heap: Heap,
    fuel: Fuel,
) -> Result<(ObjectOutcome, Heap, Fuel), Error> {
    match member {
        ObjectMember::Property {
            key, value, kind, ..
        } => match kind {
            ObjectPropertyKind::Init | ObjectPropertyKind::Method => {
                eval_data_member(key, value, acc, env, heap, fuel)
            }
            ObjectPropertyKind::Get => {
                eval_accessor_member(key, value, acc, AccessorHalf::Get, env, heap, fuel)
            }
            ObjectPropertyKind::Set => {
                eval_accessor_member(key, value, acc, AccessorHalf::Set, env, heap, fuel)
            }
        },
        ObjectMember::Spread { argument } => {
            eval(argument, env, heap, fuel).map(|(out, heap, fuel)| match out {
                Outcome::Throw(v) => (ObjectOutcome::Throw(v), heap, fuel),
                Outcome::Normal(v) => match v {
                    Value::Object(id) => {
                        let extension = heap
                            .object(id)
                            .map(|obj| obj.properties().clone())
                            .unwrap_or_default();
                        (
                            ObjectOutcome::Acc(acc.with_spread_data(extension)),
                            heap,
                            fuel,
                        )
                    }
                    Value::Undefined
                    | Value::Null
                    | Value::Boolean(_)
                    | Value::Number(_)
                    | Value::String(_)
                    | Value::Function(_)
                    | Value::Native(_)
                    | Value::Promise(_) => (ObjectOutcome::Acc(acc.clone()), heap, fuel),
                },
            })
        }
    }
}

#[derive(Clone, Copy)]
enum AccessorHalf {
    Get,
    Set,
}

fn eval_data_member(
    key: &PropertyKey,
    value: &Expression,
    acc: &ObjectAcc,
    env: &Env,
    heap: Heap,
    fuel: Fuel,
) -> Result<(ObjectOutcome, Heap, Fuel), Error> {
    eval_property_key(key, env, heap, fuel).and_then(|(key_out, heap, fuel)| match key_out {
        Outcome::Throw(v) => Ok((ObjectOutcome::Throw(v), heap, fuel)),
        Outcome::Normal(key_value) => {
            let key_str = to_property_key(&key_value, &heap);
            eval(value, env, heap, fuel).map(|(val_out, heap, fuel)| match val_out {
                Outcome::Throw(v) => (ObjectOutcome::Throw(v), heap, fuel),
                Outcome::Normal(v) => (ObjectOutcome::Acc(acc.with_data(key_str, v)), heap, fuel),
            })
        }
    })
}

fn eval_accessor_member(
    key: &PropertyKey,
    value: &Expression,
    acc: &ObjectAcc,
    half: AccessorHalf,
    env: &Env,
    heap: Heap,
    fuel: Fuel,
) -> Result<(ObjectOutcome, Heap, Fuel), Error> {
    eval_property_key(key, env, heap, fuel).and_then(|(key_out, heap, fuel)| match key_out {
        Outcome::Throw(v) => Ok((ObjectOutcome::Throw(v), heap, fuel)),
        Outcome::Normal(key_value) => {
            let key_str = to_property_key(&key_value, &heap);
            eval(value, env, heap, fuel).map(|(val_out, heap, fuel)| match val_out {
                Outcome::Throw(v) => (ObjectOutcome::Throw(v), heap, fuel),
                Outcome::Normal(v) => {
                    let extended = match half {
                        AccessorHalf::Get => acc.with_get(key_str, v),
                        AccessorHalf::Set => acc.with_set(key_str, v),
                    };
                    (ObjectOutcome::Acc(extended), heap, fuel)
                }
            })
        }
    })
}

fn eval_property_key(key: &PropertyKey, env: &Env, heap: Heap, fuel: Fuel) -> EvalResult {
    match key {
        PropertyKey::Identifier(id) => Ok((
            Outcome::Normal(Value::String(id.as_str().to_owned())),
            heap,
            fuel,
        )),
        PropertyKey::String(s) => Ok((Outcome::Normal(Value::String(s.clone())), heap, fuel)),
        PropertyKey::Number(n) => Ok((Outcome::Normal(Value::Number(*n)), heap, fuel)),
        PropertyKey::Computed(expr) => eval(expr, env, heap, fuel),
        PropertyKey::Private(_) => Err(Error::Unsupported {
            feature: "private property key",
        }),
    }
}

fn eval_member(
    object: &Expression,
    property: &MemberProperty,
    _optional: bool,
    env: &Env,
    heap: Heap,
    fuel: Fuel,
) -> EvalResult {
    step(eval(object, env, heap, fuel), |obj, heap, fuel| {
        resolve_member(&obj, property, env, heap, fuel)
    })
}

fn resolve_member(
    object: &Value,
    property: &MemberProperty,
    env: &Env,
    heap: Heap,
    fuel: Fuel,
) -> EvalResult {
    match property {
        MemberProperty::Identifier(id) => access_property(object, id.as_str(), heap, fuel),
        MemberProperty::Computed(expr) => step(eval(expr, env, heap, fuel), |key, heap, fuel| {
            let key_str = to_property_key(&key, &heap);
            access_property(object, &key_str, heap, fuel)
        }),
        MemberProperty::Private(_) => Err(Error::Unsupported {
            feature: "private member access",
        }),
    }
}

fn access_property(object: &Value, key: &str, heap: Heap, fuel: Fuel) -> EvalResult {
    match object {
        Value::Object(id) => {
            // v0.3 dispatches to a getter when `key` resolves to an
            // accessor property: the getter is invoked with
            // `this = object` and `args = []`, and its result
            // becomes the property read's value.  A getter-less
            // accessor reads as `undefined` per ECMAScript spec.
            let resolved = heap.object(*id).map(|obj| {
                obj.get(key).cloned().map_or_else(
                    || PropertyLookup::Accessor(obj.accessor(key).cloned()),
                    PropertyLookup::Data,
                )
            });
            match resolved {
                Some(PropertyLookup::Data(v)) => Ok((Outcome::Normal(v), heap, fuel)),
                Some(PropertyLookup::Accessor(Some(pair))) => match pair.get_fn().cloned() {
                    Some(getter) => call_function(&getter, object, Vec::new(), heap, fuel),
                    None => Ok((Outcome::Normal(Value::Undefined), heap, fuel)),
                },
                Some(PropertyLookup::Accessor(None)) | None => {
                    Ok((Outcome::Normal(Value::Undefined), heap, fuel))
                }
            }
        }
        Value::String(s) => Ok((string_member(s, key), heap, fuel)),
        Value::Promise(_) => Ok((promise_member(key), heap, fuel)),
        Value::Undefined
        | Value::Null
        | Value::Boolean(_)
        | Value::Number(_)
        | Value::Function(_)
        | Value::Native(_) => Ok((
            Outcome::Throw(type_error(&format!(
                "cannot access property {key:?} of non-object"
            ))),
            heap,
            fuel,
        )),
    }
}

/// v0.4: surface `.then` / `.catch` as native callables when JS
/// accesses them on a [`Value::Promise`].  Other keys resolve to
/// `undefined` per spec (Promises don't expose data properties).
fn promise_member(key: &str) -> Outcome {
    match key {
        "then" => Outcome::Normal(Value::Native(crate::promise::then_impl)),
        "catch" => Outcome::Normal(Value::Native(crate::promise::catch_impl)),
        _other => Outcome::Normal(Value::Undefined),
    }
}

enum PropertyLookup {
    Data(Value),
    Accessor(Option<crate::value::AccessorPair>),
}

fn string_member(s: &str, key: &str) -> Outcome {
    if key == "length" {
        let length = u32::try_from(s.chars().count()).unwrap_or(u32::MAX);
        Outcome::Normal(Value::Number(f64::from(length)))
    } else {
        key.parse::<usize>()
            .ok()
            .and_then(|i| s.chars().nth(i))
            .map_or(Outcome::Normal(Value::Undefined), |c| {
                Outcome::Normal(Value::String(c.to_string()))
            })
    }
}

fn eval_call(
    callee: &Expression,
    arguments: &[Expression],
    _optional: bool,
    env: &Env,
    heap: Heap,
    fuel: Fuel,
) -> EvalResult {
    eval_callee_and_this(callee, env, heap, fuel).and_then(
        |(callee_outcome, this_value, heap, fuel)| match callee_outcome {
            Outcome::Throw(v) => Ok((Outcome::Throw(v), heap, fuel)),
            Outcome::Normal(callee_value) => {
                eval_arguments(arguments, env, heap, fuel).and_then(|(args_outcome, heap, fuel)| {
                    match args_outcome {
                        ArgsOutcome::Throw(v) => Ok((Outcome::Throw(v), heap, fuel)),
                        ArgsOutcome::Values(args) => {
                            call_function(&callee_value, &this_value, args, heap, fuel)
                        }
                    }
                })
            }
        },
    )
}

fn eval_callee_and_this(
    callee: &Expression,
    env: &Env,
    heap: Heap,
    fuel: Fuel,
) -> Result<(Outcome, Value, Heap, Fuel), Error> {
    match callee.value() {
        ExpressionKind::Member {
            object, property, ..
        } => {
            eval(object, env, heap, fuel).and_then(|(obj_outcome, heap, fuel)| match obj_outcome {
                Outcome::Throw(v) => Ok((Outcome::Throw(v), Value::Undefined, heap, fuel)),
                Outcome::Normal(obj) => resolve_member(&obj, property, env, heap, fuel)
                    .map(|(member_outcome, heap, fuel)| (member_outcome, obj, heap, fuel)),
            })
        }
        _other => eval(callee, env, heap, fuel)
            .map(|(outcome, heap, fuel)| (outcome, Value::Undefined, heap, fuel)),
    }
}

enum ArgsOutcome {
    Values(Vec<Value>),
    Throw(Value),
}

fn eval_arguments(
    args: &[Expression],
    env: &Env,
    heap: Heap,
    fuel: Fuel,
) -> Result<(ArgsOutcome, Heap, Fuel), Error> {
    collect_arguments(args, 0, Vec::new(), env, heap, fuel)
}

fn collect_arguments(
    args: &[Expression],
    idx: usize,
    acc: Vec<Value>,
    env: &Env,
    heap: Heap,
    fuel: Fuel,
) -> Result<(ArgsOutcome, Heap, Fuel), Error> {
    args.get(idx).map_or_else(
        || Ok((ArgsOutcome::Values(acc.clone()), heap.clone(), fuel)),
        |arg| match arg.value() {
            ExpressionKind::Spread { argument } => eval(argument, env, heap.clone(), fuel)
                .and_then(|(out, heap, fuel)| match out {
                    Outcome::Throw(v) => Ok((ArgsOutcome::Throw(v), heap, fuel)),
                    Outcome::Normal(spread) => {
                        let items = collect_spread_items(&spread, &heap);
                        let extended: Vec<Value> = acc.clone().into_iter().chain(items).collect();
                        collect_arguments(args, idx + 1, extended, env, heap, fuel)
                    }
                }),
            _other => eval(arg, env, heap.clone(), fuel).and_then(|(out, heap, fuel)| match out {
                Outcome::Throw(v) => Ok((ArgsOutcome::Throw(v), heap, fuel)),
                Outcome::Normal(v) => {
                    let extended = append_value(acc.clone(), v);
                    collect_arguments(args, idx + 1, extended, env, heap, fuel)
                }
            }),
        },
    )
}

pub(crate) fn call_function(
    callee: &Value,
    this_value: &Value,
    args: Vec<Value>,
    heap: Heap,
    fuel: Fuel,
) -> EvalResult {
    match callee {
        Value::Function(id) => heap.function(*id).cloned().map_or_else(
            || {
                Ok((
                    Outcome::Throw(type_error("function id missing from heap")),
                    heap.clone(),
                    fuel,
                ))
            },
            |def| invoke_function(&def, this_value, args, heap.clone(), fuel),
        ),
        Value::Native(native_fn) => native_fn(args, this_value.clone(), heap, fuel),
        Value::Undefined
        | Value::Null
        | Value::Boolean(_)
        | Value::Number(_)
        | Value::String(_)
        | Value::Object(_)
        | Value::Promise(_) => Ok((
            Outcome::Throw(type_error("called value is not a function")),
            heap,
            fuel,
        )),
    }
}

fn invoke_function(
    def: &FunctionDef,
    this_value: &Value,
    args: Vec<Value>,
    heap: Heap,
    fuel: Fuel,
) -> EvalResult {
    let env_with_this = if def.is_arrow() {
        def.captured_env().clone()
    } else {
        def.captured_env()
            .extend_direct("__this__", this_value.clone())
    };
    bind_parameters(def.params(), args, env_with_this, heap, fuel).and_then(
        |(bind_outcome, env_after, heap, fuel)| match bind_outcome {
            Outcome::Throw(v) => Ok((Outcome::Throw(v), heap, fuel)),
            Outcome::Normal(_) => {
                crate::statement::execute_body(def.body(), &env_after, heap, fuel)
                    .map(|(completion, heap, fuel)| (completion_to_outcome(completion), heap, fuel))
            }
        },
    )
}

fn completion_to_outcome(completion: crate::completion::Completion) -> Outcome {
    match completion {
        crate::completion::Completion::Normal(v) | crate::completion::Completion::Return(v) => {
            Outcome::Normal(v)
        }
        crate::completion::Completion::Throw(v) => Outcome::Throw(v),
        crate::completion::Completion::Break | crate::completion::Completion::Continue => {
            Outcome::Normal(Value::Undefined)
        }
    }
}

fn bind_parameters(
    params: &[Pattern],
    args: Vec<Value>,
    base_env: Env,
    heap: Heap,
    fuel: Fuel,
) -> Result<(Outcome, Env, Heap, Fuel), Error> {
    bind_parameters_recursive(params, &args, 0, base_env, heap, fuel)
}

fn bind_parameters_recursive(
    params: &[Pattern],
    args: &[Value],
    idx: usize,
    env: Env,
    heap: Heap,
    fuel: Fuel,
) -> Result<(Outcome, Env, Heap, Fuel), Error> {
    if let Some(param) = params.get(idx) {
        let value = args.get(idx).cloned().unwrap_or(Value::Undefined);
        bind_pattern(param, value, env, heap, fuel).and_then(|(outcome, env_after, heap, fuel)| {
            match outcome {
                Outcome::Throw(v) => Ok((Outcome::Throw(v), env_after, heap, fuel)),
                Outcome::Normal(_) => {
                    bind_parameters_recursive(params, args, idx + 1, env_after, heap, fuel)
                }
            }
        })
    } else {
        Ok((Outcome::Normal(Value::Undefined), env, heap, fuel))
    }
}

fn bind_pattern(
    param: &Pattern,
    value: Value,
    env: Env,
    heap: Heap,
    fuel: Fuel,
) -> Result<(Outcome, Env, Heap, Fuel), Error> {
    use ecma_syntax_cat::pattern::PatternKind;
    match param.value() {
        PatternKind::Identifier(id) => {
            let (cell_id, heap) = heap.alloc_cell(Cell::new(value, true));
            Ok((
                Outcome::Normal(Value::Undefined),
                env.extend_cell(id.as_str(), cell_id),
                heap,
                fuel,
            ))
        }
        PatternKind::Assignment { left, right } => {
            if matches!(value, Value::Undefined) {
                eval(right, &env, heap, fuel).and_then(|(outcome, heap, fuel)| match outcome {
                    Outcome::Throw(v) => Ok((Outcome::Throw(v), env, heap, fuel)),
                    Outcome::Normal(default_value) => {
                        bind_pattern(left, default_value, env, heap, fuel)
                    }
                })
            } else {
                bind_pattern(left, value, env, heap, fuel)
            }
        }
        PatternKind::Rest { .. } | PatternKind::Array { .. } | PatternKind::Object { .. } => {
            Ok((Outcome::Normal(Value::Undefined), env, heap, fuel))
        }
    }
}

fn eval_new(
    callee: &Expression,
    arguments: &[Expression],
    env: &Env,
    heap: Heap,
    fuel: Fuel,
) -> EvalResult {
    step(eval(callee, env, heap, fuel), |callee_val, heap, fuel| {
        eval_arguments(arguments, env, heap, fuel).and_then(|(args_outcome, heap, fuel)| {
            match args_outcome {
                ArgsOutcome::Throw(v) => Ok((Outcome::Throw(v), heap, fuel)),
                ArgsOutcome::Values(args) => construct(&callee_val, args, heap, fuel),
            }
        })
    })
}

fn construct(callee: &Value, args: Vec<Value>, heap: Heap, fuel: Fuel) -> EvalResult {
    let (instance_id, heap) = heap.alloc_object(Object::empty());
    let this_value = Value::Object(instance_id);
    call_function(callee, &this_value, args, heap, fuel).map(|(outcome, heap, fuel)| {
        let final_value = match &outcome {
            Outcome::Normal(returned) => match returned {
                Value::Object(_) | Value::Function(_) => returned.clone(),
                _other => this_value.clone(),
            },
            Outcome::Throw(_) => this_value.clone(),
        };
        match outcome {
            Outcome::Throw(v) => (Outcome::Throw(v), heap, fuel),
            Outcome::Normal(_) => (Outcome::Normal(final_value), heap, fuel),
        }
    })
}

fn eval_update(
    operator: UpdateOperator,
    argument: &Expression,
    prefix: bool,
    env: &Env,
    heap: Heap,
    fuel: Fuel,
) -> EvalResult {
    step(eval(argument, env, heap, fuel), |before, heap, fuel| {
        let before_num = to_number(&before);
        let delta: f64 = match operator {
            UpdateOperator::Increment => 1.0,
            UpdateOperator::Decrement => -1.0,
        };
        let after_num = before_num + delta;
        let after = Value::Number(after_num);
        write_back_target(argument, after.clone(), env, heap, fuel).map(
            |(write_outcome, heap, fuel)| match write_outcome {
                Outcome::Throw(v) => (Outcome::Throw(v), heap, fuel),
                Outcome::Normal(_) => {
                    let result = if prefix {
                        after.clone()
                    } else {
                        Value::Number(before_num)
                    };
                    (Outcome::Normal(result), heap, fuel)
                }
            },
        )
    })
}

fn write_back_target(
    target: &Expression,
    value: Value,
    env: &Env,
    heap: Heap,
    fuel: Fuel,
) -> EvalResult {
    match target.value() {
        ExpressionKind::Identifier(id) => write_identifier(id.as_str(), value, env, heap, fuel),
        ExpressionKind::Member {
            object, property, ..
        } => step(eval(object, env, heap, fuel), |obj, heap, fuel| {
            write_member(&obj, property, value.clone(), env, heap, fuel)
        }),
        _other => Ok((
            Outcome::Throw(type_error("invalid assignment target")),
            heap,
            fuel,
        )),
    }
}

fn write_identifier(name: &str, value: Value, env: &Env, heap: Heap, fuel: Fuel) -> EvalResult {
    match env.lookup(name) {
        Some(Binding::Cell(id)) => {
            let id = *id;
            heap.store_cell(id, value.clone()).map_or_else(
                |err_heap| {
                    Ok((
                        Outcome::Throw(type_error(&format!(
                            "assignment to constant or missing cell {name:?}"
                        ))),
                        err_heap,
                        fuel,
                    ))
                },
                |new_heap| Ok((Outcome::Normal(value), new_heap, fuel)),
            )
        }
        Some(Binding::Direct(_)) => Ok((
            Outcome::Throw(type_error(&format!(
                "cannot assign to non-cell binding {name:?}"
            ))),
            heap,
            fuel,
        )),
        None => Ok((Outcome::Throw(reference_error(name)), heap, fuel)),
    }
}

fn write_member(
    object: &Value,
    property: &MemberProperty,
    value: Value,
    env: &Env,
    heap: Heap,
    fuel: Fuel,
) -> EvalResult {
    match property {
        MemberProperty::Identifier(id) => {
            store_object_member(object, id.as_str(), value, heap, fuel)
        }
        MemberProperty::Computed(expr) => step(eval(expr, env, heap, fuel), |key, heap, fuel| {
            let key_str = to_property_key(&key, &heap);
            store_object_member(object, &key_str, value.clone(), heap, fuel)
        }),
        MemberProperty::Private(_) => Err(Error::Unsupported {
            feature: "private member write",
        }),
    }
}

fn store_object_member(
    object: &Value,
    key: &str,
    value: Value,
    heap: Heap,
    fuel: Fuel,
) -> EvalResult {
    match object {
        Value::Object(id) => match heap.object(*id).cloned() {
            Some(obj) => store_property_or_invoke_setter(*id, &obj, object, key, value, heap, fuel),
            None => Ok((
                Outcome::Throw(type_error("object missing from heap")),
                heap,
                fuel,
            )),
        },
        Value::Undefined
        | Value::Null
        | Value::Boolean(_)
        | Value::Number(_)
        | Value::String(_)
        | Value::Function(_)
        | Value::Native(_)
        | Value::Promise(_) => Ok((
            Outcome::Throw(type_error("cannot set property on non-object")),
            heap,
            fuel,
        )),
    }
}

fn store_property_or_invoke_setter(
    object_id: crate::value::ObjectId,
    obj: &Object,
    object: &Value,
    key: &str,
    value: Value,
    heap: Heap,
    fuel: Fuel,
) -> EvalResult {
    // v0.3 dispatches `obj.key = v` to a setter when `key` is an
    // accessor property: the setter is invoked with `this = object`
    // and `args = [v]`, and the assignment expression evaluates to
    // `v` regardless of the setter's own return value (ECMAScript
    // spec).  A setter-less accessor silently discards the
    // assignment in non-strict mode (we don't implement strict
    // mode in v0.3, so silent is the right default).
    match obj.accessor(key).and_then(|pair| pair.set_fn().cloned()) {
        Some(setter) => call_function(&setter, object, vec![value.clone()], heap, fuel).map(
            |(outcome, heap, fuel)| match outcome {
                Outcome::Normal(_) => (Outcome::Normal(value), heap, fuel),
                Outcome::Throw(v) => (Outcome::Throw(v), heap, fuel),
            },
        ),
        None if obj.accessor(key).is_some() => Ok((Outcome::Normal(value), heap, fuel)),
        None => {
            let updated = obj.with(key.to_owned(), value.clone());
            heap.store_object(object_id, updated).map_or_else(
                |err_heap| {
                    Ok((
                        Outcome::Throw(type_error("object store failed")),
                        err_heap,
                        fuel,
                    ))
                },
                |new_heap| Ok((Outcome::Normal(value), new_heap, fuel)),
            )
        }
    }
}

fn eval_unary(
    operator: UnaryOperator,
    argument: &Expression,
    env: &Env,
    heap: Heap,
    fuel: Fuel,
) -> EvalResult {
    match operator {
        UnaryOperator::Delete => Err(Error::Unsupported {
            feature: "delete operator",
        }),
        _other => step(eval(argument, env, heap, fuel), |v, heap, fuel| {
            apply_unary(operator, &v, &heap).map(|result| (Outcome::Normal(result), heap, fuel))
        }),
    }
}

fn eval_binary(
    operator: BinaryOperator,
    left: &Expression,
    right: &Expression,
    env: &Env,
    heap: Heap,
    fuel: Fuel,
) -> EvalResult {
    step(eval(left, env, heap, fuel), |lhs, heap, fuel| {
        step(eval(right, env, heap, fuel), |rhs, heap, fuel| {
            apply_binary(operator, &lhs, &rhs, &heap)
                .map(|result| (Outcome::Normal(result), heap, fuel))
        })
    })
}

fn eval_logical(
    operator: ecma_syntax_cat::operator::LogicalOperator,
    left: &Expression,
    right: &Expression,
    env: &Env,
    heap: Heap,
    fuel: Fuel,
) -> EvalResult {
    use ecma_syntax_cat::operator::LogicalOperator;
    step(eval(left, env, heap, fuel), |lhs, heap, fuel| {
        let shortcircuit = match operator {
            LogicalOperator::And => !to_boolean(&lhs),
            LogicalOperator::Or => to_boolean(&lhs),
            LogicalOperator::NullishCoalescing => !matches!(lhs, Value::Null | Value::Undefined),
        };
        if shortcircuit {
            Ok((Outcome::Normal(lhs), heap, fuel))
        } else {
            eval(right, env, heap, fuel)
        }
    })
}

fn eval_conditional(
    test: &Expression,
    consequent: &Expression,
    alternate: &Expression,
    env: &Env,
    heap: Heap,
    fuel: Fuel,
) -> EvalResult {
    step(eval(test, env, heap, fuel), |t, heap, fuel| {
        if to_boolean(&t) {
            eval(consequent, env, heap, fuel)
        } else {
            eval(alternate, env, heap, fuel)
        }
    })
}

fn eval_assignment(
    operator: AssignmentOperator,
    left: &Expression,
    right: &Expression,
    env: &Env,
    heap: Heap,
    fuel: Fuel,
) -> EvalResult {
    match operator {
        AssignmentOperator::Assign => step(eval(right, env, heap, fuel), |rhs, heap, fuel| {
            write_back_target(left, rhs, env, heap, fuel)
        }),
        _other => step(eval(left, env, heap, fuel), |lhs, heap, fuel| {
            step(eval(right, env, heap, fuel), |rhs, heap, fuel| {
                let binary_op = compound_to_binary(operator)?;
                let combined = apply_binary(binary_op, &lhs, &rhs, &heap)?;
                write_back_target(left, combined, env, heap, fuel)
            })
        }),
    }
}

fn compound_to_binary(op: AssignmentOperator) -> Result<BinaryOperator, Error> {
    match op {
        AssignmentOperator::AddAssign => Ok(BinaryOperator::Add),
        AssignmentOperator::SubtractAssign => Ok(BinaryOperator::Subtract),
        AssignmentOperator::MultiplyAssign => Ok(BinaryOperator::Multiply),
        AssignmentOperator::DivideAssign => Ok(BinaryOperator::Divide),
        AssignmentOperator::RemainderAssign => Ok(BinaryOperator::Remainder),
        AssignmentOperator::ExponentiationAssign => Ok(BinaryOperator::Exponentiation),
        AssignmentOperator::LeftShiftAssign => Ok(BinaryOperator::LeftShift),
        AssignmentOperator::RightShiftAssign => Ok(BinaryOperator::RightShift),
        AssignmentOperator::UnsignedRightShiftAssign => Ok(BinaryOperator::UnsignedRightShift),
        AssignmentOperator::BitwiseOrAssign => Ok(BinaryOperator::BitwiseOr),
        AssignmentOperator::BitwiseXorAssign => Ok(BinaryOperator::BitwiseXor),
        AssignmentOperator::BitwiseAndAssign => Ok(BinaryOperator::BitwiseAnd),
        AssignmentOperator::Assign => Err(Error::Unsupported {
            feature: "compound_to_binary called on plain `=` (engine bug)",
        }),
        AssignmentOperator::LogicalOrAssign
        | AssignmentOperator::LogicalAndAssign
        | AssignmentOperator::NullishCoalescingAssign => Err(Error::Unsupported {
            feature: "logical-assignment operators (||=, &&=, ??=)",
        }),
    }
}

fn eval_sequence(expressions: &[Expression], env: &Env, heap: Heap, fuel: Fuel) -> EvalResult {
    eval_sequence_recursive(expressions, 0, Value::Undefined, env, heap, fuel)
}

fn eval_sequence_recursive(
    expressions: &[Expression],
    idx: usize,
    last: Value,
    env: &Env,
    heap: Heap,
    fuel: Fuel,
) -> EvalResult {
    expressions.get(idx).map_or_else(
        || Ok((Outcome::Normal(last.clone()), heap.clone(), fuel)),
        |expr| {
            step(eval(expr, env, heap.clone(), fuel), |v, heap, fuel| {
                eval_sequence_recursive(expressions, idx + 1, v, env, heap, fuel)
            })
        },
    )
}

#[allow(clippy::unnecessary_wraps)]
fn eval_arrow_function(
    arrow: &ecma_syntax_cat::function::ArrowFunction,
    env: &Env,
    heap: Heap,
    fuel: Fuel,
) -> EvalResult {
    let def = FunctionDef::new(
        None,
        arrow.params().to_vec(),
        FunctionBody::Arrow(Box::new(arrow.body().clone())),
        env.clone(),
        true,
    );
    let (id, heap) = heap.alloc_function(def);
    Ok((Outcome::Normal(Value::Function(id)), heap, fuel))
}

#[allow(clippy::unnecessary_wraps)]
fn eval_function_expression(
    func: &ecma_syntax_cat::function::Function,
    env: &Env,
    heap: Heap,
    fuel: Fuel,
) -> EvalResult {
    let def = FunctionDef::new(
        func.id().cloned(),
        func.params().to_vec(),
        FunctionBody::Statements(func.body().to_vec()),
        env.clone(),
        false,
    );
    let (id, heap) = heap.alloc_function(def);
    Ok((Outcome::Normal(Value::Function(id)), heap, fuel))
}

fn type_error(message: &str) -> Value {
    Value::String(format!("TypeError: {message}"))
}

fn reference_error(name: &str) -> Value {
    Value::String(format!("ReferenceError: {name} is not defined"))
}
