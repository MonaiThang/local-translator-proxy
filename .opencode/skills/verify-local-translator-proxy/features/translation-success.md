# Translation success

A player sees a fully translated Translation Unit: every non-blank line is
translated, every Rendering Token appears byte-for-byte in its original order,
and every physical line ending is copied from the source.

## Sub-features

- `single-line` translates one supported line and returns it as plain UTF-8 text.
- `multi-line` translates each non-blank line and reassembles the unit.
- `separators` preserves `\r\n`, `\r`, and `\n` exactly where the source had them.
- `tokens` keeps every `<...>` and `[RRGGBB]`/`[-]` Rendering Token unchanged.
- `blank-lines` leaves blank physical lines out of the model calls and in place.

## How to get to it (user POV)

- The game sends `GET /?text=<unit>&from=zh-CN&to=en` to the proxy's loopback
  address after capturing on-screen text.
- The player reads the returned text in the game at the same place as the
  source text; no extra UI exists in the proxy.

## Driving it with verify.ps1

Preconditions:

- The current tree builds with the GNU toolchain.
- Ports `5099` and `7331` are free (or pass `-ProxyPort`/`-StubPort`).
- The stub runs in `translate` mode, which prefixes `EN:` to each line.

- **Single line.** Run
  `pwsh -NoProfile -File .opencode/skills/verify-local-translator-proxy/scripts/verify.ps1 -Scenario translate`.
  The summary reports status `200`, `text/plain; charset=utf-8`, body `EN:你好`,
  and exactly one model call.
- **Multi-line with tokens.** Run the same helper with
  `-Scenario multi-line-tokens`. The request is `你好\r\n<b>世界</b>`; the body
  hex contains `0D0A` between the lines, both `<b>` and `</b>` survive, and the
  summary reports exactly two model calls.
- **Blank line in the middle.** From a manual run, request
  `text=你好\r\n\r\n世界` and confirm the empty middle line is returned
  unchanged and only two model calls appear in `GET /requests` on the stub.
- **Proof.** Read the run's `summary.json`, `body-1.bin`, and `response-1.json`.
  The response record contains the URL, status, content type, elapsed ms, body
  as UTF-8, and body hex.

## Gotchas

- The console prints CJK as `??`. Compare `bodyHex` or reread `body-1.bin`; do
  not conclude the response is corrupt from the terminal.
- The stub's `EN:` prefix is verification scaffolding, not expected product
  output. Assert it only in verification.
- A line with a malformed known tag (`<b class="x">`, unclosed `[FF7700`) is an
  expected passthrough, not a translation success. Use the token tests and
  `passthrough-safety.md`.
- `from`/`to` default to `zh-CN`/`en` and must match the allowlist in
  `config.toml`. An unlisted pair is a passthrough by design.
- Do not launch on port `5000`; the user's game may own it.
