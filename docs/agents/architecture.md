# Architecture Context

Read this before changing `src/` or the integration tests. Domain vocabulary is
defined in `GLOSSARY.md`; binding decisions live in `docs/adr/`.

## One request, one unit

The game sends one `GET` carrying a Translation Unit in `text`. The proxy
returns plain text and always status `200`. **Passthrough-first**: if any step
cannot be completed safely, return the original unit byte-for-byte rather than
an error. This keeps the game from stalling.

```text
GET /any/path?text=...&from=...&to=...
  └─ http::handle                       lenient query parse, 200 + text/plain
      └─ pipeline::Pipeline::translate
          ├─ language::resolve_pair     allowlist gate
          ├─ lines::split_lines         keep exact separators
          ├─ tokens::extract (source)   ambiguity → passthrough
          ├─ JoinSet + Semaphore        one model call per non-blank line
          │   └─ runtime::RuntimeClient::translate_line
          ├─ tokens::matches            positional equality per line
          ├─ reject injected \r / \n
          ├─ timeout_at(unit deadline)  whole unit, not per call
          └─ reassembly                 original separators
```

## Module map

| Module | Responsibility | Key entry points |
|---|---|---|
| `src/lib.rs` | Wires config, runtime client, pipeline, and router | `build_router` |
| `src/main.rs` | Process lifecycle, logging, graceful shutdown | `main` |
| `src/config.rs` | TOML + env overrides, normalization, validation | `Config::load`, `validate` |
| `src/language.rs` | Tag normalization and allowlist resolution | `resolve_pair` |
| `src/http.rs` | Any-path handler and lenient query parsing | `handle`, `parse_query` |
| `src/lines.rs` | Split/rejoin preserving `\r\n`, `\r`, `\n` | `split_lines`, `reassemble` |
| `src/tokens.rs` | Rendering Token scan and positional match | `extract`, `matches` |
| `src/runtime.rs` | Ollama-compatible `/api/chat` client | `translate_line` |
| `src/pipeline.rs` | Per-unit orchestration, deadline, concurrency | `Pipeline::translate` |

## Invariants to preserve

- A failed line fails the whole unit; never mix translated and original lines.
- Separators are copied from the source; a translated line must not contain a
  real `\r` or `\n`.
- Tokens must match the source positionally: same text, order, and count.
- `{text}`, whitespace-only units, and unsupported/malformed language pairs make
  zero model calls.
- `timeout_secs` bounds the whole Translation Unit, including permit waits,
  model calls, and verification.
- `max_in_flight` caps concurrent Model Runtime calls across the process.
- Model calls are stateless: no retries and no caching.

## Where to change what

- **Token set**: `src/tokens.rs` (`parse_tag`, `looks_like_known_tag`, tests).
- **Prompt shape**: `RuntimeClient::system_message` and the `PromptConfig`
  default.
- **Config keys and env names**: `src/config.rs`, `config.example.toml`,
  `README.md`, `docs/spec/v1.md`.
- **Failure policy**: `Pipeline::translate` and `Pipeline::run_unit`.
- **Wire format**: `src/runtime.rs` (`ChatRequest`, `parse_response`).

The full contract is in `docs/spec/v1.md`.
