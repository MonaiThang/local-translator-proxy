---
name: verify-token-scanner
description: Formally-style verify the Rendering Token scanner in src/tokens.rs — every generated input must produce in-bounds, ordered, exact source slices or a located ambiguity error, and never a panic. Use when a change touches token parsing, bracket/angle-tag recognition, escaped line breaks, or the passthrough-on-ambiguity path.
---

# Verify the Rendering Token scanner

**Property.** For every input string, `tokens::extract` never panics; it either
returns an error whose offset is a char boundary pointing at a tag opener, or
returns tokens that are exact, in-order, non-overlapping slices of the source.

**Harm if broken.** A panic escapes the inbound handler and breaks the
always-`200` game contract, stalling the game; a wrong offset or slice makes the
pipeline compare the wrong text and can let a translation that corrupts on-screen
markup through.

The check runs the real scanner in `src/tokens.rs` against generated input. It
is a property test, not a mathematical proof: scope is the generated families
below, 512 cases each at the pinned proptest version.

## Why this tool

No practical formal verifier runs on this repository's Windows GNU toolchain
(Kani and the Rust proof assistants target other hosts). Per the pstack formal
verification guidance, the real code is exercised with generated input instead
of a hand-built model. `proptest` is pinned in `Cargo.lock`, needs no external
service, and shrinks failures to a minimal input.

## Run it

```powershell
$env:PATH = "C:\msys64\mingw64\bin;" + $env:PATH
cargo +stable-x86_64-pc-windows-gnu test --test formal_tokens
```

Deepen a run without editing the test:

```powershell
$env:PROPTEST_CASES = "5000"
cargo +stable-x86_64-pc-windows-gnu test --test formal_tokens
```

## Read the result

- **Pass.** `2 passed`. Both generated families held the property at this case
  count. This does not prove all strings, and says nothing about translation
  quality or the rest of the pipeline.
- **Fail — `left: None` / `right: Some(...)`, or "not the source slice".** A real
  scanner bug: `extract` reported a token whose `start`/`text` do not match the
  source. The minimal failing input is part of the output; reproduce it with the
  unit tests or a one-off request. Do not weaken the property.
- **Fail — "overlaps or is out of order".** A real ordering bug: two tokens share
  bytes or are emitted out of source order. This breaks positional matching with
  the model's answer.
- **Fail — "does not start with a token opener" or "is not a supported escaped
  break".** A real classification bug: the scanner invented a token shape or
  captured a `\` sequence the pipeline cannot verify.
- **Fail — "error offset ... does not point at a tag opener" or "is not a char
  boundary".** A real location bug: the ambiguity error cannot be turned into a
  precise log line, and a malformed tag may be missed instead of triggering a
  Passthrough Response.
- **Panic.** A real out-of-bounds or bad-slice bug in the scanner. The panic
  message and input are the counterexample.
- **A failure caused by the test itself** (a new strategy that produces a shape
  the scanner is documented to accept, or an assertion that assumes more than
  the module promises) is a modeling error. Fix the test only when the module
  contract says the input is genuinely in scope; record why in this file.

## Coverage record

Verified code, at commit `50aada9047b029199e46ffe6c40621e43d7d7c9a` (the HEAD
when this check was generated; the test file and dev-dependency were added
afterward and are not yet committed):

| File | Functions | What the property covers |
|---|---|---|
| `src/tokens.rs` | `extract`, `parse_tag`, `parse_bracket_tag`, `parse_escaped_break`, `looks_like_known_tag`, `looks_like_known_bracket_tag` | Every parser branch the two generated families reach: closing tags, `<br>` forms, paired tags, `<color>`/`<size>`, `<sprite>`, `<quad>`, six-hex NGUI brackets, `[-]`, malformed known tags, escaped `\r\n`/`\r`/`\n`, arbitrary Unicode. |

Not covered: `tokens::matches` (covered by unit tests), UTF-16 or non-UTF-8 input
(the wire type is `String`), and any behavior downstream of `extract`.

Before moving the sync commit forward, confirm this file still describes
`src/tokens.rs` at the new commit. If `extract` or any parser above changed,
rerun the check and update the table before recording the new commit.

## Prove the check still bites

When the test or the scanner changes materially, re-confirm the check can fail:

1. In `src/tokens.rs`, change one `start: index` on a `tokens.push(Token { ... })`
   to `start: index + 1`.
2. Run the command above.
3. Expect `tag_corpus_keeps_the_scanner_contract` to fail with a minimal input
   such as `<color=red>` and `left: None`, `right: Some("<color=red>")`.
4. Revert the change and confirm `2 passed` again. Delete
   `tests/formal_tokens.proptest-regressions` if the scratch run created it.

That exact failure was observed when this skill was generated, on
2026-11-14, from commit `50aada9`.

## What this does not cover

- That the pipeline turns an `Err` into a Passthrough Response, that separators
  survive reassembly, or that the deadline holds. Those are runtime behaviors:
  use `verify-local-translator-proxy`.
- Model output. The property is about the source scanner only.
- Inputs larger than the generator's size bound (64 chars for arbitrary text,
  16 fragments for the corpus). Raise the bound in `tests/formal_tokens.rs` if a
  change makes long-input structure matter, then rerun.
