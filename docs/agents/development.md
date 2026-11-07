# Development Context

Read this before building, testing, or manually verifying the proxy. Basic
commands are in `README.md`; this file records the environment and test
knowledge the repo does not encode.

## Toolchain on this machine

The default `RUSTUP_TOOLCHAIN` points at an MSVC toolchain without `link.exe`.
Build with the GNU toolchain and put MSYS2's binutils on `PATH` first:

```powershell
$env:PATH = "C:\msys64\mingw64\bin;" + $env:PATH
cargo +stable-x86_64-pc-windows-gnu test
```

PowerShell sometimes prints `Exited with code 1` while cargo still says
`Finished` and every test passes. Trust cargo's output and `$LASTEXITCODE`.

## Checks

Run all three before committing:

```powershell
cargo +stable-x86_64-pc-windows-gnu fmt --all -- --check
cargo +stable-x86_64-pc-windows-gnu clippy --all-targets -- -D warnings
cargo +stable-x86_64-pc-windows-gnu test
```

## Test layout

- `tests/common/mod.rs` — configurable mock Model Runtime plus router, request,
  and concurrency helpers. `Behavior` selects echo, prefix, replace, status,
  delay, newline injection, or conditional failure/replacement.
- `tests/http_contract.rs` — inbound parsing and response envelope.
- `tests/pipeline.rs` — end-to-end translation, token safety, passthrough,
  concurrency cap, and the unit deadline.
- `tests/runtime.rs` — the HTTP client against a local mock server: request
  shape, response envelopes, and error mapping.
- Module unit tests cover pure logic (parsing, scanners, config, and tokens).

Add behavior-level tests to `tests/pipeline.rs`; keep scanners and parsers in
unit tests beside their module.

## Manual smoke test

1. Start the proxy with a closed runtime endpoint and a short deadline:

   ```powershell
   $env:TRANSLATOR_PROXY_SERVER_LISTEN_ADDR = "127.0.0.1:5099"
   $env:TRANSLATOR_PROXY_MODEL_RUNTIME_ENDPOINT = "http://127.0.0.1:1/api/chat"
   $env:TRANSLATOR_PROXY_MODEL_RUNTIME_TIMEOUT_SECS = "1"
   cargo run
   ```

2. Request a unit and confirm status, content type, and original bytes:

   ```powershell
   curl.exe -s -o body.txt -w "%{http_code} %{content_type}" `
     "http://127.0.0.1:5099/?text=%E4%BD%A0%E5%A5%BD&from=zh-CN&to=en"
   [System.IO.File]::ReadAllBytes("body.txt")
   ```

   The bytes for `你好` are `e4 bd a0 e5 a5 bd`. Console rendering may show
   `??`; inspect the bytes, not the console.

For real translation, point the endpoint at Ollama and use `$env:RUST_LOG =
"debug"` to see per-unit timing and failure reasons.

## Conventions

- One commit per issue, with the issue number in the subject.
- Keep the game contract (`127.0.0.1:5000`, `text`/`from`/`to`) unchanged.
- `config.toml` is tracked; edit it directly for local defaults. Use
  `config.example.toml` for documented values.
- New behavior gets an integration test covering both the success path and the
  passthrough path.
