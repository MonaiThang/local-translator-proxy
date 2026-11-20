# Passthrough safety

When translation cannot be supplied safely, the player sees the original
Translation Unit byte-for-byte, the response is still `200 OK`, and the game
never stalls on an error or a half-translated unit.

## Sub-features

- `runtime-unreachable` returns the original unit when the Model Runtime is closed.
- `runtime-timeout` returns the original unit when the runtime misses the budget.
- `token-mismatch` returns the original unit when a model answer drops, reorders,
  or adds a Rendering Token and the span retry cannot rebuild a safe line.
- `injected-newline` returns the original unit when a model answer contains a
  real `\r` or `\n`.
- `token-malformed` returns the original unit when a source line has an ambiguous
  known tag.
- `never-mixed` never returns some translated lines and some original lines.

## How to get to it (user POV)

- Any game request whose translation cannot be verified follows this path.
- The player keeps reading the source text, correctly rendered; nothing in the
  game treats the request as a failure.

## Driving it with verify.ps1

Preconditions:

- The current tree builds with the GNU toolchain.
- Ports `5099` and `7331` are free, or pass alternatives.
- The helper starts the stub in the mode each scenario names.
- The helper disables the startup warm-up for the passthrough scenarios, so a
  failed or slow runtime is not confused with a failed startup call. The
  `passthrough-runtime-down` scenario is the exception: it keeps warm-up on to
  prove the proxy serves after a failed warm-up.

- **Unreachable runtime.** Run
  `pwsh -NoProfile -File .opencode/skills/verify-local-translator-proxy/scripts/verify.ps1 -Scenario passthrough-runtime-down`.
  Warm-up fails against the closed endpoint and logs `warm-up failed`; the
  request still gets `200`, content type `text/plain; charset=utf-8`, and the
  original `你好` bytes.
- **Timeout.** Run the helper with `-Scenario passthrough-timeout`. The stub
  delays 3000 ms, the proxy budget is 1 s, and the summary shows the original
  bytes with `elapsedMs` below 2500.
- **Token mismatch.** Run the helper with `-Scenario passthrough-mismatch`.
  The source is `<b>你好</b>`; the stub drops both tags on the whole line and
  adds `[FF0000]` to every answer, so the span retry also fails token matching.
  The response is the original `<b>你好</b>` and two model calls were attempted.
- **Injected newline.** Run the helper with
  `-Scenario passthrough-injected-newline`. The response bytes contain no `0A`.
- **Malformed source token.** Start a manual run with
  `TRANSLATOR_PROXY_MODEL_RUNTIME_WARM_UP=false`, request
  `text=<b class="x">你好`, and confirm the body is the original bytes with zero
  model calls (the ambiguous token is found before any call).
- **Proof.** `summary.json` records each check, `body-N.bin` holds the exact
  bytes, and `proxy.log` shows the rejection reason at `warn`.

## Gotchas

- A passthrough must be byte-identical, including separators and tokens. A
  trimmed or re-encoded response is a failure even if it looks the same.
- `passthrough-mismatch` needs a line that actually has a token. The stub's
  mismatch mode drops tokens on the whole line and adds an extra token to every
  answer, so the passthrough survives recovery.
- The unreachable scenario deliberately runs no stub. Do not point it at a real
  Ollama instance; a slow real model would test the timeout path instead.
- The proxy logs a failed span recovery at `warn` with the reason. Read
  `proxy.log` to tell token drift from an injected newline. A whole-line
  token rejection that will be retried is logged at `debug`.
- The unreachable scenario also logs the failed startup warm-up at `warn`.
  That line appears before the request and is not a request-level failure.
- Malformed-token suppression is covered by `cargo test` today, not by a helper
  scenario; drive it manually with `{text}` as above. Disable warm-up first if
  you are counting model calls.
