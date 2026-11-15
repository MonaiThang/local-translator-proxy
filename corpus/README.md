# zh-CN to en Translation Corpus

`zh-en.json` holds the Translation Corpus used to compare Model Runtime models
for issue #20. It exercises game dialogue, punctuation, Rendering Tokens, and
the line shapes the proxy must return safely. The comparison script replays
every entry through the real proxy and prints one report column per model.

## Schema

`zh-en.json` is one object.

| Field | Type | Meaning |
|---|---|---|
| `version` | number | Corpus format version. Currently `1`. |
| `from` | string | Source language sent to the proxy. |
| `to` | string | Target language sent to the proxy. |
| `entries` | array | Between 20 and 40 Translation Units. |

Each entry has these fields.

| Field | Type | Meaning |
|---|---|---|
| `id` | string | Unique kebab-case identifier. |
| `category` | string | One category from the table below. |
| `source` | string | One physical zh-CN line. |
| `reference` | string | Human English reference translation. |
| `must_preserve` | array | Rendering Tokens that must appear in this order. Empty when the line has none. |
| `notes` | string, optional | Why the line is in the corpus. |

## Categories

| Category | What the entry exercises |
|---|---|
| `dialogue` | NPC speech, quest text, and natural sentence flow. |
| `punctuation` | Full-width punctuation, quotes, ellipsis, and dashes. |
| `ui` | Short interface labels and status messages. |
| `combat` | Barks, damage text, and status effects. |
| `item` | Item names, descriptions, and material lore. |
| `colour` | NGUI square-bracket colour tags such as `[FF7700]` and `[-]`. |
| `markup` | UGUI and TextMeshPro tags such as `<b>`, `<color=...>`, `<size=...>`, and `<sprite .../>`. |
| `line-break` | Literal `\n` Rendering Tokens inside one physical line. |
| `square-bracket` | The issue #18 case, where a model may escape brackets as `\u005b`. |

The corpus must contain at least one entry in `dialogue`, `punctuation`,
`colour`, `markup`, `line-break`, and `square-bracket`.

## Invariants

- `entries` has at least 20 items and every `id` is unique.
- `source` is one physical line. A real CR or LF would split the Translation
  Unit, so the entry would no longer isolate a single model call.
- Every `must_preserve` string appears in both `source` and `reference`, in the
  same order.
- `reference` is an evaluation target for human review. It is not required to
  match a model's output word for word.

`cargo test --test corpus` checks these rules and the required categories.

## Provenance

Every line is synthetic game text written for this repository. No line comes
from a shipped game, and no reference is production copy. The references record
one acceptable English rendering, not the only one.

The comparison keeps the shipped JSON-envelope prompt so the parser, token
verification, and passthrough contract stay unchanged. TranslateGemma's own
plain-text prompt format is out of scope for #20.

## Run the comparison

Install the default model, then run the script from the repository root.

```powershell
ollama pull translategemma:4b
pwsh ./scripts/compare-models.ps1
```

Pass `-Models translategemma:4b,gemma2:9b` to compare several installed models
side by side. Every model in the list must already be installed.

| Option | Default | Meaning |
|---|---|---|
| `-Models` | `translategemma:4b` | Models to compare. |
| `-CorpusPath` | `corpus/zh-en.json` | Corpus to replay. |
| `-WarmupPasses` | `1` | Discarded passes that load the model and warm the runtime. |
| `-MeasuredPasses` | `5` | Timed passes over the corpus. |
| `-PortBase` | `5101` | First loopback port for the proxy processes. |
| `-TimeoutSecs` | `5` | Proxy deadline for one Translation Unit. |
| `-OutputDir` | `target/model-comparison` | Report output directory. |
| `-ProxyExe` | `target/debug/local-translator-proxy.exe` | Proxy binary to run. |
| `-OllamaEndpoint` | `http://127.0.0.1:11434` | Model Runtime endpoint. |

The script runs one model at a time. A single Model Runtime keeps one model
resident, so interleaving the sides would measure model loads rather than
translation. Each side gets a discarded warm-up pass before the timed passes.

## Read the report

The report has one row per corpus entry and one summary row per model.

- `translated` means the proxy returned a translated line. The proxy verifies
  the JSON schema and the Rendering Tokens positionally before it does, so a
  mismatch would have produced a Passthrough Response instead.
- `passthrough` means the response equals the source. The summary breaks down
  the proxy log reasons, such as timeout, runtime error, or token mismatch.
- `preserved` reports the `must_preserve` check performed by the script. The
  proxy's positional token verification is authoritative.
- Latency is wall time through the proxy, reported as a median and range over
  the measured passes. Treat a gap smaller than the run-to-run spread as noise.
