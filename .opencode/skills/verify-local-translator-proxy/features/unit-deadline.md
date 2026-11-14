# Unit deadline

One wall-clock budget covers the whole Translation Unit: every permit wait,
every model call, and token verification. A slow or stuck line cannot extend it,
and a miss returns the original unit.

## Sub-features

- `whole-unit` starts the deadline once per Translation Unit, not per line.
- `slow-runtime` returns the original unit when the runtime outlasts the budget.
- `no-mixed-output` never returns partially translated output after a timeout.
- `still-200` keeps the game-safe `200 OK` and plain-text content type.

## How to get to it (user POV)

- The game waits at most the configured budget for any one Translation Unit.
- A player sees either fully translated text or the original text, never a
  stalled request or a half-translated unit.

## Driving it with verify.ps1

Preconditions:

- The current tree builds with the GNU toolchain.
- Ports `5099` and `7331` are free, or pass alternatives.
- The stub is configured with `STUB_DELAY_MS=3000` and the proxy with
  `TRANSLATOR_PROXY_MODEL_RUNTIME_TIMEOUT_SECS=1`; the helper does both.

- **Timeout path.** Run
  `pwsh -NoProfile -File .opencode/skills/verify-local-translator-proxy/scripts/verify.ps1 -Scenario passthrough-timeout`.
  Status is `200`, the body is the original `你好` bytes, and `elapsedMs` is
  under 2500.
- **Tighten the bound manually.** Repeat a manual launch with
  `TRANSLATOR_PROXY_MODEL_RUNTIME_TIMEOUT_SECS=1` and a stub delay of 5000 ms.
  Confirm `proxy.log` contains `translation unit timed out` at `warn`.
- **Proof.** `response-1.json` records `elapsedMs`; `proxy.log` records the
  timeout warning; `summary.json` records both checks.

## Gotchas

- The proxy's HTTP client uses the same `timeout_secs` as its per-request
  timeout, so a hung runtime can surface as a runtime timeout before the unit
  deadline. Both paths return the original unit; the warning text differs.
- Do not assert an exact elapsed time. Assert "well under the stub delay" and
  "at or above roughly the budget"; CI scheduling adds noise.
- The deadline is seconds-granular in configuration. A 0-second budget is not a
  supported test shortcut.
- A timeout proof is not a concurrency proof. Peak in-flight is covered by
  `tests/pipeline.rs`.
