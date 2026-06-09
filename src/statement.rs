//! Statement evaluation.

use ecma_syntax_cat::declaration::{VariableDeclaration, VariableDeclarator};
use ecma_syntax_cat::expression::Expression;
use ecma_syntax_cat::function::{ArrowBody, Function};
use ecma_syntax_cat::pattern::PatternKind;
use ecma_syntax_cat::statement::{CatchClause, ForInit, Statement, StatementKind};

use crate::coercion::to_boolean;
use crate::completion::Completion;
use crate::env::Env;
use crate::error::Error;
use crate::expression::eval;
use crate::fuel::Fuel;
use crate::heap::Heap;
use crate::outcome::Outcome;
use crate::value::{Cell, FunctionBody, FunctionDef, Value};
use ecma_syntax_cat::declaration::VariableKind;

/// The result of evaluating a statement: a completion paired with the new
/// heap, env, and fuel.
pub type StmtResult = Result<(Completion, Heap, Env, Fuel), Error>;

/// The result of evaluating a function body.  No env change is exposed
/// (the caller's env is unchanged after a call returns).
pub type BodyResult = Result<(Completion, Heap, Fuel), Error>;

/// Evaluate a single statement.
///
/// # Errors
///
/// Propagates fatal [`Error`] conditions.
pub fn eval_statement(statement: &Statement, env: &Env, heap: Heap, fuel: Fuel) -> StmtResult {
    let fuel = fuel.spend()?;
    match statement.value() {
        StatementKind::Block { body } => eval_block(body, env, heap, fuel),
        StatementKind::Empty | StatementKind::Debugger => Ok((
            Completion::Normal(Value::Undefined),
            heap,
            env.clone(),
            fuel,
        )),
        StatementKind::Expression { expression } => {
            eval_expression_statement(expression, env, heap, fuel)
        }
        StatementKind::If {
            test,
            consequent,
            alternate,
        } => eval_if(test, consequent, alternate.as_deref(), env, heap, fuel),
        StatementKind::While { test, body } => eval_while(test, body, env, heap, fuel),
        StatementKind::DoWhile { body, test } => eval_do_while(body, test, env, heap, fuel),
        StatementKind::For {
            init,
            test,
            update,
            body,
        } => eval_for(
            init.as_ref(),
            test.as_ref(),
            update.as_ref(),
            body,
            env,
            heap,
            fuel,
        ),
        StatementKind::Return { argument } => eval_return(argument.as_ref(), env, heap, fuel),
        StatementKind::Throw { argument } => eval_throw(argument, env, heap, fuel),
        StatementKind::Try {
            block,
            handler,
            finalizer,
        } => eval_try(block, handler.as_ref(), finalizer.as_ref(), env, heap, fuel),
        StatementKind::Break { label: _ } => Ok((Completion::Break, heap, env.clone(), fuel)),
        StatementKind::Continue { label: _ } => Ok((Completion::Continue, heap, env.clone(), fuel)),
        StatementKind::VariableDeclaration(decl) => {
            eval_variable_declaration(decl, env, heap, fuel)
        }
        StatementKind::FunctionDeclaration(func) => {
            eval_function_declaration(func, env, heap, fuel)
        }
        StatementKind::Switch { .. } => Err(Error::Unsupported {
            feature: "switch statement",
        }),
        StatementKind::ForIn { .. } => Err(Error::Unsupported {
            feature: "for-in statement",
        }),
        StatementKind::ForOf { .. } => Err(Error::Unsupported {
            feature: "for-of statement",
        }),
        StatementKind::Labeled { .. } => Err(Error::Unsupported {
            feature: "labeled statement",
        }),
        StatementKind::ClassDeclaration(_) => Err(Error::Unsupported {
            feature: "class declaration",
        }),
    }
}

/// Evaluate a statement list as a block.
///
/// Sequential evaluation; abrupt completions short-circuit.
///
/// # Errors
///
/// Propagates fatal [`Error`].
pub fn eval_block(body: &[Statement], env: &Env, heap: Heap, fuel: Fuel) -> StmtResult {
    let body_env = env.clone();
    eval_statements_sequential(body, 0, Value::Undefined, &body_env, heap, fuel)
        .map(|(completion, heap, _inner_env, fuel)| (completion, heap, env.clone(), fuel))
}

fn eval_statements_sequential(
    body: &[Statement],
    idx: usize,
    last: Value,
    env: &Env,
    heap: Heap,
    fuel: Fuel,
) -> StmtResult {
    if let Some(stmt) = body.get(idx) {
        eval_statement(stmt, env, heap, fuel).and_then(|(completion, heap, next_env, fuel)| {
            if completion.is_abrupt() {
                Ok((completion, heap, next_env, fuel))
            } else {
                let value = match completion {
                    Completion::Normal(v) => v,
                    Completion::Return(_)
                    | Completion::Throw(_)
                    | Completion::Break
                    | Completion::Continue => last.clone(),
                };
                eval_statements_sequential(body, idx + 1, value, &next_env, heap, fuel)
            }
        })
    } else {
        Ok((Completion::Normal(last), heap, env.clone(), fuel))
    }
}

fn eval_expression_statement(
    expression: &Expression,
    env: &Env,
    heap: Heap,
    fuel: Fuel,
) -> StmtResult {
    eval(expression, env, heap, fuel).map(|(outcome, heap, fuel)| {
        let completion = match outcome {
            Outcome::Normal(v) => Completion::Normal(v),
            Outcome::Throw(v) => Completion::Throw(v),
        };
        (completion, heap, env.clone(), fuel)
    })
}

fn eval_if(
    test: &Expression,
    consequent: &Statement,
    alternate: Option<&Statement>,
    env: &Env,
    heap: Heap,
    fuel: Fuel,
) -> StmtResult {
    eval(test, env, heap, fuel).and_then(|(outcome, heap, fuel)| match outcome {
        Outcome::Throw(v) => Ok((Completion::Throw(v), heap, env.clone(), fuel)),
        Outcome::Normal(v) => {
            if to_boolean(&v) {
                eval_statement(consequent, env, heap, fuel)
            } else if let Some(alt) = alternate {
                eval_statement(alt, env, heap, fuel)
            } else {
                Ok((
                    Completion::Normal(Value::Undefined),
                    heap,
                    env.clone(),
                    fuel,
                ))
            }
        }
    })
}

fn eval_while(
    test: &Expression,
    body: &Statement,
    env: &Env,
    heap: Heap,
    fuel: Fuel,
) -> StmtResult {
    eval(test, env, heap, fuel).and_then(|(test_out, heap, fuel)| match test_out {
        Outcome::Throw(v) => Ok((Completion::Throw(v), heap, env.clone(), fuel)),
        Outcome::Normal(test_val) => {
            if to_boolean(&test_val) {
                eval_statement(body, env, heap, fuel).and_then(
                    |(body_completion, heap, _env_after, fuel)| match body_completion {
                        Completion::Break => Ok((
                            Completion::Normal(Value::Undefined),
                            heap,
                            env.clone(),
                            fuel,
                        )),
                        Completion::Return(_) | Completion::Throw(_) => {
                            Ok((body_completion, heap, env.clone(), fuel))
                        }
                        Completion::Continue | Completion::Normal(_) => {
                            eval_while(test, body, env, heap, fuel)
                        }
                    },
                )
            } else {
                Ok((
                    Completion::Normal(Value::Undefined),
                    heap,
                    env.clone(),
                    fuel,
                ))
            }
        }
    })
}

fn eval_do_while(
    body: &Statement,
    test: &Expression,
    env: &Env,
    heap: Heap,
    fuel: Fuel,
) -> StmtResult {
    eval_statement(body, env, heap, fuel).and_then(|(body_completion, heap, _env_after, fuel)| {
        match body_completion {
            Completion::Break => Ok((
                Completion::Normal(Value::Undefined),
                heap,
                env.clone(),
                fuel,
            )),
            Completion::Return(_) | Completion::Throw(_) => {
                Ok((body_completion, heap, env.clone(), fuel))
            }
            Completion::Continue | Completion::Normal(_) => eval_while(test, body, env, heap, fuel),
        }
    })
}

fn eval_for(
    init: Option<&ForInit>,
    test: Option<&Expression>,
    update: Option<&Expression>,
    body: &Statement,
    env: &Env,
    heap: Heap,
    fuel: Fuel,
) -> StmtResult {
    let (loop_env, heap, fuel) = init.map_or_else(
        || Ok::<_, Error>((env.clone(), heap.clone(), fuel)),
        |init_form| match init_form {
            ForInit::Declaration(decl) => eval_variable_declaration(decl, env, heap.clone(), fuel)
                .map(|(_completion, heap, env_after, fuel)| (env_after, heap, fuel)),
            ForInit::Expression(expr) => eval(expr, env, heap.clone(), fuel)
                .map(|(_outcome, heap, fuel)| (env.clone(), heap, fuel)),
        },
    )?;
    eval_for_loop(test, update, body, &loop_env, heap, fuel)
        .map(|(completion, heap, _final_env, fuel)| (completion, heap, env.clone(), fuel))
}

fn eval_for_loop(
    test: Option<&Expression>,
    update: Option<&Expression>,
    body: &Statement,
    env: &Env,
    heap: Heap,
    fuel: Fuel,
) -> StmtResult {
    let (proceed, heap, fuel) = test.map_or_else(
        || Ok::<_, Error>((true, heap.clone(), fuel)),
        |t| {
            eval(t, env, heap.clone(), fuel).map(|(out, heap, fuel)| match out {
                Outcome::Throw(_) => (false, heap, fuel),
                Outcome::Normal(v) => (to_boolean(&v), heap, fuel),
            })
        },
    )?;
    if proceed {
        eval_statement(body, env, heap, fuel).and_then(|(body_completion, heap, _env, fuel)| {
            match body_completion {
                Completion::Break => Ok((
                    Completion::Normal(Value::Undefined),
                    heap,
                    env.clone(),
                    fuel,
                )),
                Completion::Return(_) | Completion::Throw(_) => {
                    Ok((body_completion, heap, env.clone(), fuel))
                }
                Completion::Continue | Completion::Normal(_) => {
                    let (heap, fuel) = update.map_or_else(
                        || Ok::<_, Error>((heap.clone(), fuel)),
                        |u| eval(u, env, heap.clone(), fuel).map(|(_out, heap, fuel)| (heap, fuel)),
                    )?;
                    eval_for_loop(test, update, body, env, heap, fuel)
                }
            }
        })
    } else {
        Ok((
            Completion::Normal(Value::Undefined),
            heap,
            env.clone(),
            fuel,
        ))
    }
}

fn eval_return(argument: Option<&Expression>, env: &Env, heap: Heap, fuel: Fuel) -> StmtResult {
    argument.map_or_else(
        || {
            Ok((
                Completion::Return(Value::Undefined),
                heap.clone(),
                env.clone(),
                fuel,
            ))
        },
        |expr| {
            eval(expr, env, heap.clone(), fuel).map(|(outcome, heap, fuel)| {
                let completion = match outcome {
                    Outcome::Normal(v) => Completion::Return(v),
                    Outcome::Throw(v) => Completion::Throw(v),
                };
                (completion, heap, env.clone(), fuel)
            })
        },
    )
}

fn eval_throw(argument: &Expression, env: &Env, heap: Heap, fuel: Fuel) -> StmtResult {
    eval(argument, env, heap, fuel).map(|(outcome, heap, fuel)| {
        let completion = match outcome {
            Outcome::Normal(v) | Outcome::Throw(v) => Completion::Throw(v),
        };
        (completion, heap, env.clone(), fuel)
    })
}

fn eval_try(
    block: &[Statement],
    handler: Option<&CatchClause>,
    finalizer: Option<&Vec<Statement>>,
    env: &Env,
    heap: Heap,
    fuel: Fuel,
) -> StmtResult {
    let block_result = eval_block(block, env, heap, fuel)?;
    let after_catch = run_catch_phase(block_result, handler, env)?;
    run_finalizer(finalizer, after_catch, env)
}

fn run_catch_phase(
    block_result: (Completion, Heap, Env, Fuel),
    handler: Option<&CatchClause>,
    env: &Env,
) -> StmtResult {
    let (completion, heap, env_after, fuel) = block_result;
    if let Completion::Throw(thrown) = completion {
        if let Some(clause) = handler {
            run_catch_clause(clause, thrown, env, heap, fuel)
        } else {
            Ok((Completion::Throw(thrown), heap, env_after, fuel))
        }
    } else {
        Ok((completion, heap, env_after, fuel))
    }
}

fn run_catch_clause(
    clause: &CatchClause,
    thrown: Value,
    env: &Env,
    heap: Heap,
    fuel: Fuel,
) -> StmtResult {
    let (catch_env, heap) = if let Some(pattern) = clause.param() {
        match pattern.value() {
            PatternKind::Identifier(id) => {
                let (cell_id, heap) = heap.alloc_cell(Cell::new(thrown, true));
                (env.extend_cell(id.as_str(), cell_id), heap)
            }
            _other => (env.clone(), heap),
        }
    } else {
        (env.clone(), heap)
    };
    eval_block(clause.body(), &catch_env, heap, fuel)
        .map(|(completion, heap, _inner_env, fuel)| (completion, heap, env.clone(), fuel))
}

fn run_finalizer(
    finalizer: Option<&Vec<Statement>>,
    after_catch: (Completion, Heap, Env, Fuel),
    env: &Env,
) -> StmtResult {
    let (completion, heap, env_after, fuel) = after_catch;
    if let Some(fin) = finalizer {
        eval_block(fin, env, heap, fuel).map(|(fin_completion, heap, _inner_env, fuel)| {
            let final_completion = if fin_completion.is_abrupt() {
                fin_completion
            } else {
                completion.clone()
            };
            (final_completion, heap, env_after.clone(), fuel)
        })
    } else {
        Ok((completion, heap, env_after, fuel))
    }
}

fn eval_variable_declaration(
    decl: &VariableDeclaration,
    env: &Env,
    heap: Heap,
    fuel: Fuel,
) -> StmtResult {
    let mutable = matches!(decl.kind(), VariableKind::Var | VariableKind::Let);
    eval_declarators(decl.declarators(), 0, env.clone(), heap, fuel, mutable)
}

fn eval_declarators(
    decls: &[VariableDeclarator],
    idx: usize,
    env: Env,
    heap: Heap,
    fuel: Fuel,
    mutable: bool,
) -> StmtResult {
    if let Some(d) = decls.get(idx) {
        let (id_kind, default_init): (
            NormalisedId,
            Option<&ecma_syntax_cat::expression::Expression>,
        ) = normalise_declarator(d);
        let resolved_init = d.init().or(default_init);
        let init_result = resolved_init.map_or_else(
            || Ok::<_, Error>((Outcome::Normal(Value::Undefined), heap.clone(), fuel)),
            |init_expr| eval(init_expr, &env, heap.clone(), fuel),
        );
        init_result.and_then(|(outcome, heap, fuel)| match outcome {
            Outcome::Throw(v) => Ok((Completion::Throw(v), heap, env.clone(), fuel)),
            Outcome::Normal(value) => match id_kind {
                NormalisedId::Identifier(name) => {
                    let (cell_id, heap) = heap.alloc_cell(Cell::new(value, mutable));
                    let new_env = env.extend_cell(name, cell_id);
                    eval_declarators(decls, idx + 1, new_env, heap, fuel, mutable)
                }
                NormalisedId::Other => Err(Error::Unsupported {
                    feature: "destructuring in variable declarations",
                }),
            },
        })
    } else {
        Ok((Completion::Normal(Value::Undefined), heap, env, fuel))
    }
}

enum NormalisedId {
    Identifier(String),
    Other,
}

fn normalise_declarator(
    d: &VariableDeclarator,
) -> (
    NormalisedId,
    Option<&ecma_syntax_cat::expression::Expression>,
) {
    match d.id().value() {
        PatternKind::Identifier(id) => (NormalisedId::Identifier(id.as_str().to_owned()), None),
        PatternKind::Assignment { left, right } => match left.value() {
            PatternKind::Identifier(id) => (
                NormalisedId::Identifier(id.as_str().to_owned()),
                Some(right.as_ref()),
            ),
            _other => (NormalisedId::Other, None),
        },
        _other => (NormalisedId::Other, None),
    }
}

fn eval_function_declaration(func: &Function, env: &Env, heap: Heap, fuel: Fuel) -> StmtResult {
    let name = func
        .id()
        .map_or_else(String::new, |i| i.as_str().to_owned());
    let (cell_id, heap) = heap.alloc_cell(Cell::new(Value::Undefined, true));
    let new_env = env.extend_cell(name.clone(), cell_id);
    let def = FunctionDef::new(
        func.id().cloned(),
        func.params().to_vec(),
        FunctionBody::Statements(func.body().to_vec()),
        new_env.clone(),
        false,
        func.is_async(),
    );
    let (fn_id, heap) = heap.alloc_function(def);
    heap.store_cell(cell_id, Value::Function(fn_id))
        .map_or_else(
            |err_heap| {
                Ok((
                    Completion::Throw(Value::String(format!(
                        "TypeError: failed to bind function {name}"
                    ))),
                    err_heap,
                    new_env.clone(),
                    fuel,
                ))
            },
            |final_heap| {
                Ok((
                    Completion::Normal(Value::Undefined),
                    final_heap,
                    new_env.clone(),
                    fuel,
                ))
            },
        )
}

/// Evaluate a function body in `env`.  Used by [`crate::expression`] to
/// invoke functions; the env change is not exposed.
///
/// # Errors
///
/// Propagates fatal [`Error`].
pub fn execute_body(body: &FunctionBody, env: &Env, heap: Heap, fuel: Fuel) -> BodyResult {
    match body {
        FunctionBody::Statements(stmts) => eval_block(stmts, env, heap, fuel)
            .map(|(completion, heap, _env, fuel)| (completion, heap, fuel)),
        FunctionBody::Arrow(arrow) => match arrow.as_ref() {
            ArrowBody::Expression(expr) => {
                eval(expr, env, heap, fuel).map(|(outcome, heap, fuel)| {
                    let completion = match outcome {
                        Outcome::Normal(v) => Completion::Return(v),
                        Outcome::Throw(v) => Completion::Throw(v),
                    };
                    (completion, heap, fuel)
                })
            }
            ArrowBody::Block(stmts) => eval_block(stmts, env, heap, fuel)
                .map(|(completion, heap, _env, fuel)| (completion, heap, fuel)),
        },
    }
}
