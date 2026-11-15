# Startup warm-up

When `model_runtime.warm_up` is true (the tracked default), the proxy translates
one short line at startup so the game's first Translation Unit does not pay the
model's cold-load time. Warm-up is best effort: a failure is logged at `warn`
and the proxy serves anyway.

## Sub-features

- `startup-call` sends exactly one model call with the short line `你好`.
- `completion-log` logs `warm-up completed` at `info` after the call succeeds.
- `best-effort` starts and serves after a warm-up failure, logging
  `warm-up failed` at `warn`.
- `not-a-unit` keeps warm-up outside any Translation Unit: a later request
  still makes its own model calls and sees no cached or half-warm answer.

## How to get to it (user POV)

- The user starts the proxy before the game; the model loads while the game is
  still starting.
- The player sees no warm-up UI. The visible effect is a first translated line
  that is not delayed by a cold model load.

## Driving it with verify.ps1

Preconditions:

- The current tree builds with the GNU toolchain.
- Ports `5099` and `7331` are free, or pass alternatives.
- The stub runs in `translate` mode.

- **Successful warm-up.** Run
  `pwsh -NoProfile -File .opencode/skills/verify-local-translator-proxy/scripts/verify.ps1 -Scenario warm-up`.
  The helper leaves `TRANSLATOR_PROXY_MODEL_RUNTIME_WARM_UP=true`, waits for the
  stub's first call, checks that it is `你好` and that it is the only startup
  call, waits for `warm-up completed` in `proxy.log`, then sends one game
  request and expects two calls in total.
- **Failed warm-up.** Run `-Scenario passthrough-runtime-down`. The endpoint is
  closed, warm-up is enabled, and the helper checks for `warm-up failed` in
  `proxy.log` before confirming the request still returns `200` with the
  original bytes.
- **Proof.** `stub.log` holds the warm-up line, `proxy.log` holds the completion
  or failure line, and `summary.json` records the checks.

## Gotchas

- Every other scenario sets `TRANSLATOR_PROXY_MODEL_RUNTIME_WARM_UP=false`, so
  the model-call count describes the game request alone. If you reproduce a
  scenario by hand with the tracked `config.toml`, subtract the warm-up call or
  disable warm-up first.
- Warm-up is not part of a Translation Unit deadline. It uses `timeout_secs` as
  its HTTP timeout and shares `max_in_flight` with request traffic.
- The `{text}` readiness probe does not trigger a translation, but warm-up does.
  Read `GET /requests` on the stub when a count looks off by one.
- Warm-up never retries. A transient failure leaves the model unloaded until the
  first real request.
