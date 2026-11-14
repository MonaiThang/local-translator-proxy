# Passthrough safety

When translation cannot be supplied safely, the player sees the original
Translation Unit byte-for-byte, the response is still `200 OK`, and the game
never stalls on an error or a half-translated unit.

## Sub-features

- `runtime-unreachable` returns the original unit when the Model Runtime is closed.
- `runtime-timeout` returns the original unit when the runtime misses the budget.
- `token-mismatch` returns the original unit when a model answer drops, reorders,
  or adds a Rendering Token.
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

- **Unreachable runtime.** Run
  `pwsh -NoProfile -File .opencode/skills/verify-local-translator-proxy/scripts/verify.ps1 -Scenario passthrough-runtime-down`.
  Status is `200`, content type is `text/plain; charset=utf-8`, and the body is
  the original `你好` bytes.
- **Timeout.** Run the helper with `-Scenario passthrough-timeout`. The stub
  delays 3000 ms, the proxy budget is 1 s, and the summary shows the original
  bytes with `elapsedMs` below 2500.
- **Token mismatch.** Run the helper with `-Scenario passthrough-mismatch`.
  The source is `<b>你好</b>`; the stub drops both tags; the response is the
  original `<b>你好</b>` and one model call was attempted.
- **Injected newline.** Run the helper with
  `-Scenario passthrough-injected-newline`. The response bytes contain no `0A`.
- **Malformed source token.** Start a manual run, request
  `text=<b class="x">你好`, and confirm the body is the original bytes with zero
  model calls (the ambiguous token is found before any call).
- **Proof.** `summary.json` records each check, `body-N.bin` holds the exact
  bytes, and `proxy.log` shows the rejection reason at `warn`.

## Gotchas

- A passthrough must be byte-identical, including separators and tokens. A
  trimmed or re-encoded response is a failure even if it looks the same.
- `passthrough-mismatch` needs a line that actually has a token; the stub's
  mismatch mode only changes tokenized lines.
- The unreachable scenario deliberately runs no stub. Do not point it at a real
  Ollama instance; a slow real model would test the timeout path instead.
- The proxy logs rejected model output at `warn` with the reason. Read
  `proxy.log` to tell a token mismatch from an injected newline.
- Malformed-token suppression is covered by `cargo test` today, not by a helper
  scenario; drive it manually with `{text}` as above.
