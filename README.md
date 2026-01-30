# wscrab 🦀

Rust implementation of a [wscat](https://github.com/websockets/wscat) subset (connect-only).

## Features

- Connect-only mode: `-c <url>` or `--connect <url>`
- Custom headers: `--header <header:value>` (repeatable)
- Client certificate: `--cert <path>` (PEM/DER)
- Skip certificate verification: `--no-check`
- Print ping/pong notifications: `--show-ping-pong`
- Interactive prefixing: outbound `> `, inbound `< `
- Slash commands: `--slash` to send `/ping`, `/pong`, `/close`
- Help: `--help`

## Usage

```bash
wscrab -c wss://websocket-echo.com
```

Custom header:

```bash
wscrab -c wss://websocket-echo.com --header "X-Test:hello"
```

Self-signed certificate (PEM/DER supported):

```bash
wscrab -c wss://localhost:1234 --cert ./cert.pem
```

Skip certificate verification:

```bash
wscrab -c wss://localhost:1234 --no-check
```

Print ping/pong:

```bash
wscrab -c wss://websocket-echo.com --show-ping-pong
```

Slash commands (control frames):

```bash
wscrab -c wss://websocket-echo.com --slash
```

## Run tests

```bash
cargo test
```

## Learn from the code

This is a simple but complete async Rust program. You can learn Rust fundamentals and engineering practices from it. See the [Rust Learning Guide](./the-rust-inside.md).
