# Translation Proxy verification map

This directory is the maintained source for proving the game-facing behavior of
the Translation Proxy. Read this index before driving the app, then use the
matching feature file as the recipe.

The map covers the real binary over HTTP. The Model Runtime is the only replaced
boundary: a deterministic Node stub stands in for the separately installed
Ollama process.

## Baseline preconditions

- Build with the GNU toolchain and MSYS2 binutils, per `docs/agents/development.md`:

  ```powershell
  $env:PATH = "C:\msys64\mingw64\bin;" + $env:PATH
  cargo +stable-x86_64-pc-windows-gnu build
  ```

- Run the proxy on an isolated loopback port, never the game's `127.0.0.1:5000`.
  Use `-ProxyPort 5099` (or another free port); the helper starts and stops it.
- Point `TRANSLATOR_PROXY_MODEL_RUNTIME_ENDPOINT` at the stub on an isolated
  port (`-StubPort 7331`). Never point a verification run at the user's real
  Ollama instance.
- Do not edit `config.toml`. Environment overrides are enough and keep the
  tracked defaults intact.
- Never drive an instance this run did not start. If a port is busy, choose
  another pair rather than reusing or killing the listener.
- Node 24+ (`node`) and PowerShell 7 (`pwsh`) must be on `PATH`.

## Driving conventions

- Start every recipe with
  `pwsh -NoProfile -File .opencode/skills/verify-local-translator-proxy/scripts/verify.ps1 -Scenario <name>`.
- Treat every command as literal; keep quoted Chinese text and flags unchanged.
- The game sends `GET /?text=...&from=...&to=...`; any path and verb are
  accepted. Build URLs with `[uri]::EscapeDataString($text)` and compare bytes.
- Windows terminals render CJK as `??`. Trust `body-N.bin`, `bodyHex` in
  `response-N.json`, and `summary.json`, not the console.
- The stub's model-call count is the side-effect proof: `GET /requests` on the
  stub, or the run summary.

## Proof and skip reporting

- Exercise the real binary just built, not an in-process router.
- Capture the request and the resulting response bytes, the status, the content
  type, and the model-call count.
- A Passthrough Response proof shows the original bytes and at least one model
  call (or zero calls for the no-model-call feature).
- Evidence lives in `target/verification/<timestamp>-<scenario>/` and survives
  teardown. Never delete it during cleanup.
- If a scenario cannot start, report the attempted command and the unmet
  precondition. Do not report a skipped path as verified through another one.

## Feature entry contract

Each feature file starts with an H1 title and one paragraph describing the
user-visible behavior. It then uses exactly four H2 sections in this order:
`Sub-features`, `How to get to it (user POV)`, `Driving it with verify.ps1`,
and `Gotchas`.

## Features

- [Translation success](./translation-success.md) covers single-line and
  multi-line translation, separator preservation, and Rendering Token fidelity.
- [Passthrough safety](./passthrough-safety.md) covers runtime failure, token
  mismatch, injected line breaks, and the whole-unit deadline.
- [No model call](./no-model-call.md) covers the placeholder, whitespace-only
  units, unsupported pairs, and blank lines that never reach the model.
- [Unit deadline](./unit-deadline.md) covers one wall-clock budget for the whole
  Translation Unit, including permit waits and model calls.
