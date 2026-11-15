---
name: verify-local-translator-proxy
description: Drive the real local-translator-proxy binary over HTTP and prove game-facing behavior — translation, Passthrough Responses, model-call suppression, startup warm-up, and the whole-unit deadline. Use when a change touches the inbound handler, pipeline, line/token handling, runtime client, startup, or when you need evidence that the proxy still returns 200 + plain UTF-8 text.
---

# Verify the Translation Proxy

The Translation Proxy is a loopback HTTP service. Its user is the game: one
`GET` carrying a Translation Unit in `text`, answered with `200 OK` and
`text/plain; charset=utf-8`. Verification launches the real binary, points it at
a deterministic Model Runtime stub, and drives it with the same query string the
game sends.

Read `docs/agents/architecture.md` for the invariants and `docs/agents/development.md`
for the GNU toolchain.

## Launch

Verification never uses the game's port (`127.0.0.1:5000`) and never edits
`config.toml`. Every run uses environment overrides:

```powershell
$env:PATH = "C:\msys64\mingw64\bin;" + $env:PATH
cargo +stable-x86_64-pc-windows-gnu build

# Model Runtime stub (the external boundary, replaced deterministically)
$env:STUB_PORT = "7331"; $env:STUB_MODE = "translate"; $env:STUB_DELAY_MS = "0"
node .opencode/skills/verify-local-translator-proxy/scripts/runtime-stub.mjs

# Proxy under verification
$env:TRANSLATOR_PROXY_SERVER_LISTEN_ADDR = "127.0.0.1:5099"
$env:TRANSLATOR_PROXY_MODEL_RUNTIME_ENDPOINT = "http://127.0.0.1:7331/api/chat"
$env:TRANSLATOR_PROXY_MODEL_RUNTIME_TIMEOUT_SECS = "5"
$env:TRANSLATOR_PROXY_MODEL_RUNTIME_MAX_IN_FLIGHT = "4"
# The tracked default is true. Turn it off for a manual run that counts model
# calls, so the count describes the game request; the helper does this itself
# for every scenario except `warm-up` and `passthrough-runtime-down`.
$env:TRANSLATOR_PROXY_MODEL_RUNTIME_WARM_UP = "false"
$env:RUST_LOG = "info"
target\debug\local-translator-proxy.exe
```

Ready means: `target\debug\local-translator-proxy.exe` logged
`listening addr=127.0.0.1:5099`, the stub answered `GET /health`, and a first
`GET /?text=%7Btext%7D` returned `200` with `text/plain; charset=utf-8`. The
literal `{text}` placeholder is the safe probe: it never reaches the model, so
it does not disturb the model-call counter.

When `model_runtime.warm_up` is true (the tracked default), startup also sends
one translation of `你好` to the runtime before the first game request. That call
is real, so a run that counts model calls must disable it or attribute it. The
helper disables it everywhere except `warm-up` and `passthrough-runtime-down`.

The helper does launch, readiness, drive, and teardown in one command (see
Helpers). Prefer it; use the manual commands when you need to stay attached.

## Doctor

Run this first whenever a run looks wrong:

```powershell
pwsh -NoProfile -File .opencode/skills/verify-local-translator-proxy/scripts/verify.ps1 -Scenario doctor
```

It builds the current tree, starts its own stub and proxy on isolated ports,
checks status, content type, the `{text}` placeholder, the stub's mode, and that
zero model calls were made, then prints the evidence directory. It is read-only
toward repository state. An instance started by someone else (including the
game's proxy on port 5000) is never driven.

## Drive

Scenarios, each one real user path end to end:

| Scenario | What it proves |
|---|---|
| `translate` | One supported line is translated; one model call. |
| `multi-line-tokens` | `\r\n` survives; each non-blank line is translated; `<b>` tokens survive. |
| `passthrough-mismatch` | A model answer that drops a Rendering Token yields the original unit. |
| `passthrough-injected-newline` | A model answer containing a real newline yields the original unit. |
| `passthrough-timeout` | A slow runtime yields the original unit inside the unit deadline. |
| `passthrough-runtime-down` | A closed runtime endpoint yields the original unit, still `200`; a failed startup warm-up is logged and does not stop the proxy. |
| `no-model-call` | `{text}`, whitespace-only, and unsupported pairs make zero model calls. |
| `warm-up` | Startup sends exactly one translation of `你好`, logs completion, and later requests add their own calls. |

Run one with:

```powershell
pwsh -NoProfile -File .opencode/skills/verify-local-translator-proxy/scripts/verify.ps1 -Scenario translate
```

Manual equivalent for a single drive (proxy and stub already running):

```powershell
$text = "你好"
$url = "http://127.0.0.1:5099/?text=$([uri]::EscapeDataString($text))&from=zh-CN&to=en"
curl.exe -s -o body.bin -D headers.txt $url
Get-Content headers.txt
# Expect: HTTP/1.1 200 OK and Content-Type: text/plain; charset=utf-8
# Stub MODE=translate returns "EN:你好"; compare bytes, not console rendering.
```

Inspect the bytes, not the console. Windows terminals render CJK as `??`; the
proof is `[IO.File]::ReadAllBytes("body.bin")` or the hex in `summary.json`.

## Evidence

Each run writes to `target/verification/<yyyyMMdd-HHmmss>-<scenario>/`:

- `summary.json` — every check with pass/fail and details, plus the evidence path.
- `body-N.bin` — the exact response bytes for request N.
- `response-N.json` — URL, status, content type, elapsed ms, body as UTF-8 and hex.
- `proxy.log`, `proxy.err.log`, `stub.log`, `stub.err.log`, `build.log`.

Proof standards:

- Exercise the game's real request shape (`GET`, `text`/`from`/`to`), not an
  in-process router. The binary must be the one just built.
- Assert exact bytes and the model-call count, not only the final screen.
- The Model Runtime is the only mocked boundary. The stub replaces a separately
  installed external system; everything inside the proxy is the real code.
- A passthrough proof must include the original bytes and the fact that a model
  call happened (or did not, for `no-model-call`).
- Evidence outlives teardown. Never delete it while cleaning up.

## Cleanup

The helper stops only the PIDs it started, after every success and every
failure. For a manual run, stop the two processes you started by PID
(`Stop-Process -Id <pid>`), never by image name. Scratch response files may be
removed; `target/verification/` is proof and stays.

If a run cannot start because a port is busy, choose another pair:

```powershell
pwsh -NoProfile -File .opencode/skills/verify-local-translator-proxy/scripts/verify.ps1 -Scenario translate -ProxyPort 5199 -StubPort 7431
```

## Helpers

- `scripts/verify.ps1` — launch, doctor, drive, assert, evidence, teardown.
  `-Scenario` accepts the table above; `-ProxyPort`/`-StubPort` isolate the run.
- `scripts/runtime-stub.mjs` — deterministic Ollama-compatible `/api/chat`
  stand-in. `STUB_MODE` is `translate` (prefix `EN:`), `echo`, `mismatch`
  (drops tokens), `inject` (adds a newline), or `fail` (HTTP 500).
  `GET /requests` lists every line that reached the model; `POST /reset` clears
  it; `GET /health` reports readiness.

## Coverage record

Verified against the real binary at commit
`06487c3dbc216066df747d8076b3c3934c7b5cc8` (HEAD at this sync). The previous
sync was the commit that added this skill (`f7dd0c3`), which did not record
covered code; this table starts here.

| File | What the scenarios cover |
|---|---|
| `src/http.rs` | Any-path `GET`, `text`/`from`/`to` parsing, `200` + `text/plain; charset=utf-8`. |
| `src/pipeline.rs` | Placeholder, blank, and allowlist suppression; one call per non-blank line; token and injected-line-break rejection; whole-unit deadline; startup warm-up call and its best-effort failure. |
| `src/lines.rs` | `\r\n` preservation and reassembly on translated and passthrough paths. |
| `src/tokens.rs` | Token extraction and positional match on the translated and rejected paths. |
| `src/runtime.rs` | Request/response round trip through the stub, timeout and unreachable mapping. |
| `src/main.rs` | Bind, startup warm-up spawn, serve, shutdown. |
| `src/config.rs` | The env overrides in the launch block, including `warm_up`. |

Before moving the sync commit forward, diff these files from it, read the commit
messages, and rerun every scenario. A new model call outside a Translation Unit
changes the model-call counters; a new startup step may need its own scenario.

## What this does not cover

- Translation quality or the real Ollama model. That is the Translation Corpus,
  not this skill; the stub deliberately returns deterministic text.
- The game's XUnity.AutoTranslator integration on port 5000.
- Concurrency beyond the configured cap. `multi-line-tokens` proves one call
  per non-blank line, not the peak in-flight bound (that has an integration
  test in `tests/pipeline.rs`). Warm-up's share of a permit under request load
  is likewise not exercised here.
- The latency benefit of warm-up, `num_ctx`, and `keep_alive`. This skill proves
  the warm-up call happens and is best effort; cold/warm timings are
  `scripts/bench.ps1` against a real runtime.
- Whether the runtime request carries `num_ctx` and `keep_alive`. The stub
  ignores extra fields; `tests/runtime.rs` asserts the exact request body.
