# boa-cat

Tree-walking ECMAScript interpreter, built on [`ecma-syntax-cat`](https://crates.io/crates/ecma-syntax-cat) and the rest of the parser stack ([`ecma-lex-cat`](https://crates.io/crates/ecma-lex-cat), [`ecma-parse-cat`](https://crates.io/crates/ecma-parse-cat)).

`boa-cat` is the engine layer of a `comp-cat-rs` reformulation of a JavaScript runtime targeting Tauri integration.  It consumes a parsed `Program` and evaluates it to a `Value` over a persistent `Heap`, with all the framework constraints intact: no `mut`, no `Rc`/`Arc`, no interior mutability, no panics, exhaustive matches, static dispatch.

## Example

```rust
use boa_cat::{run, Error};

fn main() -> Result<(), Error> {
    let value = run("function fact(n) { if (n <= 1) { return 1; } return n * fact(n - 1); } fact(5)").run()?;
    assert_eq!(format!("{value}"), "120");
    Ok(())
}
```

## v0 scope

- All literals (number, string, boolean, null, template).
- Variable resolution with `var` / `let` / `const`.
- All binary, unary, logical, conditional, and compound-assignment operators with ECMA-262 semantics.
- Functions: declarations, expressions, arrow forms; closures; call + return; parameter defaults.
- Member access (dot, computed, optional chain).
- Array and object literals (with spread and shorthand).
- `if`, `while`, `do-while`, `for(;;)`, `throw`, `try`/`catch`/`finally`, `return`, `break`, `continue`.
- Recursive function self-reference (via cell pre-allocation).

## Deferred to v0.2+

- Classes, modules, async/await/yield, generators, destructuring, tagged templates, labeled break/continue, `for-in`/`for-of`/`switch`, full BigInt and RegExp support.  Built-ins (`Math`, `JSON`, `console`, etc.) live in the upcoming `ecma-runtime-cat`.

## Design

Heap and environment are persistent: every operation that conceptually mutates state returns a new value, and the caller threads it forward.  Variables live in heap-allocated cells so that assignment can update a cell without rebuilding the surrounding environment, while keeping `Env` itself immutable.

The top-level `run` returns `comp_cat_rs::effect::io::Io<Error, Value>`; the actual evaluator is plain `Result` and is wrapped at the boundary.

## License

MIT OR Apache-2.0
