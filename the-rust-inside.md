# Learning Rust with wscrab (for beginners)

This document is not about business logic. It focuses on Rust fundamentals and engineering practices you can learn from this project. Read the code while following this guide.

## 1. How to read a Rust project structure

- `Cargo.toml`: dependencies and build configuration.
- `src/main.rs`: executable entry point (single binary).
- `tests/`: integration tests.

> One sentence to remember: **Cargo.toml is config, src is code, tests are verification.**

## 2. Entry point: main()

`main()` is the program entry. In this project it is async (`#[tokio::main]`).

- `Opts::parse()` parses CLI arguments.
- If `--connect` is missing, it prints help and exits.
- Otherwise it calls `run()`.

You can learn:

- Structs + derive macros (`#[derive(Parser)]`).
- A basic error-handling pattern (`if let Err(err) = ...`).

## 3. Structs and fields (type system basics)

`Opts` is a struct describing CLI options.

- `Option<String>` for optional arguments.
- `Vec<String>` for repeatable arguments.
- `bool` for flags.

Key types to understand:

- `Option<T>`: maybe a value, maybe not.
- `Vec<T>`: growable array.

## 4. Modules and `use`

Rust imports external crates with `use`.

You can learn:

- How to bring crates into scope (e.g., `tokio`, `clap`).
- How to import types from nested paths (e.g., `tokio_tungstenite::tungstenite::Message`).

Practice idea: remove a `use`, see the compiler error, then add it back.

## 5. Ownership and borrowing (the key concept)

Typical examples in this project:

- `let mut connect_url = opts.connect.unwrap();`
  - `unwrap()` moves the value out of the `Option`.
- `opts.cert.as_deref()`
  - converts `Option<PathBuf>` to `Option<&Path>` without moving ownership.

You can learn:

- **Move**: once a value is moved, the original binding can’t be used.
- **Borrow**: use a reference without taking ownership.

## 6. Error handling basics

This project mainly uses `Result`:

- `run()` returns `Result<(), Box<dyn Error>>`.
- `?` propagates errors upward.

You can learn:

- The basic `Result<T, E>` pattern.
- How `?` simplifies error propagation.

## 7. Async and concurrency (tokio)

`tokio::select!` is the key concurrency tool:

- It watches stdin, network messages, and Ctrl+C.
- Whichever event is ready is handled immediately.

You can learn (and map to code):

- `async fn`: returns a `Future` and runs when awaited.
- `await`: yields execution until the async operation completes.
- The **race** pattern of `tokio::select!`:
  - Multiple async branches are declared together.
  - **The first ready branch runs**, others are canceled for this round and recreated next round.
  - Think of it as “wait on several events, handle the first one”.

The code looks like:

```text
tokio::select! {
    line = lines.next_line() => { ... }
    msg = read.next() => { ... }
    _ = tokio::signal::ctrl_c() => { ... }
}
```

This is essentially “one event loop, three input sources.”

## 8. Enums and `match` (protocol handling)

`Message` is an enum:

- `Message::Text`, `Message::Binary`, `Message::Ping`, etc.
- A `match` handles each variant.

You can learn:

- Rust enums can carry data per variant.
- `match` must be exhaustive, so the compiler helps avoid missing cases.
- This gives **compile-time completeness**, reducing bugs.

## 9. Traits and generics (advanced but common)

`handle_message()` takes a generic parameter:

```text
write: &mut (impl SinkExt<Message, Error = ...> + Unpin)
```

You can learn:

- `impl Trait` means “any type that satisfies these traits”.
- `SinkExt<Message>` means the value can **send** WebSocket messages.
- `Unpin` means it can be safely moved in memory (important for async).

## 10. Testing (integration tests)

`tests/` is for black-box tests:

- Start a temporary server.
- Run the `wscrab` binary.
- Assert on output or captured messages.

You can learn:

- How integration tests are organized.
- How to start an async server in tests.

## 11. Practice tasks (step by step)

1. **Understand `Opts`**: rename parameters and see help output change.
2. **Change prefixes**: replace `> ` with `OUT:` and `< ` with `IN:`.
3. **Add a new flag**: `--quiet` to suppress normal output.
4. **Customize errors**: change `error: ...` to your own format.
5. **Add a test**: verify `/pong data` sends a pong frame.

## 12. Suggested learning order

- First: **Option / Result / match / ownership**.
- Then: **async/await**.
- Finally: **traits / generics**.
