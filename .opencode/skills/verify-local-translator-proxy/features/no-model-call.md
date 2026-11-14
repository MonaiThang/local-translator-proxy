# No model call

Requests that cannot or should not be translated return before reaching the
Model Runtime: the literal `{text}` placeholder, whitespace-only units,
unsupported language pairs, and blank lines inside a multi-line unit.

## Sub-features

- `placeholder` returns literal `{text}` unchanged with zero model calls.
- `whitespace` returns a whitespace-only unit unchanged with zero model calls.
- `unsupported-pair` returns the unit unchanged when `from`/`to` is not in the
  allowlist.
- `blank-lines` skips blank physical lines inside an otherwise translated unit.
- `defaults` applies `zh-CN`/`en` when `from` or `to` is missing.

## How to get to it (user POV)

- The game sends the placeholder when it has no real text, and sends whatever
  language header it has, including unsupported ones.
- A player sees the source text unchanged; no translation cost is paid for text
  that cannot be translated.

## Driving it with verify.ps1

Preconditions:

- The current tree builds with the GNU toolchain.
- Ports `5099` and `7331` are free, or pass alternatives.
- The stub runs in `translate` mode so any unexpected call is visible.

- **All suppression paths.** Run
  `pwsh -NoProfile -File .opencode/skills/verify-local-translator-proxy/scripts/verify.ps1 -Scenario no-model-call`.
  It checks `{text}`, three spaces, `ja`→`en`, and `zh-CN`→`fr`, then asserts
  `modelCalls=0`.
- **Blank line inside a translated unit.** Run a manual pair: request
  `text=你好\r\n\r\n世界` and confirm only two entries appear at
  `http://127.0.0.1:7331/requests`.
- **Defaults.** Request with no `from`/`to` parameters. The response is
  translated, proving the default pair `zh-CN`/`en` resolved.
- **Proof.** `summary.json` lists each body check and the final model-call
  count; `stub.log` is empty of request lines for the suppression run.

## Gotchas

- The readiness probe itself uses `{text}`. If the counter looks off by one,
  check whether the probe was counted; it should not be.
- `+` decodes to a space and `%2B` to a literal plus, matching the game's
  `HttpUtility.ParseQueryString`. Encode text with `[uri]::EscapeDataString`.
- A pair is allowed only when both tags match after normalization. `zh-CN`→`en`
  is the tracked default; edit `config.toml` or override
  `TRANSLATOR_PROXY_LANGUAGES_SUPPORTED_PAIRS` only if the scenario needs another
  pair, and revert it afterwards.
- Suppression is not a Passthrough Response in the failure sense: nothing was
  attempted. Do not log it as a translation failure.
