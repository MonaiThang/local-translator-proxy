# `translation_proxy.ps1` — what the script actually is

Source: `C:\Users\monai\OneDrive\Desktop\translation_proxy.ps1`, read in full (104 lines, not 92).

**Premise correction.** The task described this file as the *caller* of a Translation Proxy that does not
exist yet. It is the opposite: the script **is** a Translation Proxy. It starts an `HttpListener` on
`http://127.0.0.1:5000/` (`translation_proxy.ps1:12-14`) and, for each inbound Translation Request, calls
Google's public `translate_a/single` endpoint as its translation backend (`translation_proxy.ps1:54-57`).
The game-side caller is **not in this file** and its request shape is only observable indirectly, from what
this script parses. Both directions are documented below.

## 1. How it works, end to end

| Aspect | Finding |
|---|---|
| Entry point | Top-level script, no functions/params (`translation_proxy.ps1:1-104`). Runs immediately on invocation. |
| Dependencies | `System.Web` for `HttpUtility` (`:1`); `System.Net.HttpListener` (`:12`); `System.Text.RegularExpressions` (`:44`). |
| Side effect at start | Elevates own process priority to `High`, warning only on failure (`:4-10`). |
| Bind | `http://127.0.0.1:5000/`, all paths under that prefix (`:13`). |
| Lifecycle | Long-running blocking server loop: `while ($listener.IsListening) { $context = $listener.GetContext() }` (`:23-25`). Blocks per request; never stops or exits explicitly. Not one-shot, not polling. |
| Per-request flow | Read `RawUrl` → parse query (`:30-32`) → read `text`/`from`/`to` with defaults (`:34-36`) → reject placeholder/blank (`:39`) → split on line breaks (`:44`) → translate each non-blank line (`:46-64`) → rejoin (`:71`) → write response (`:75-78`). |
| External binaries | **None.** No `Start-Process`, no exe, no local model runtime is invoked. Translation is an HTTP call to Google (`:54-57`). |
| Native game hooks | **None** — no Unity, MelonLoader, BepInEx or TextMeshPro references anywhere (`:1-104`). This is a standalone PowerShell host. |
| File paths touched | **None.** No `Get-Content`/`Set-Content`/`Out-File` (`:1-104`). |
| Registry paths touched | **None** (`:1-104`). |
| Console output | Pretty-printed `[ORIGINAL]` / `[TRANSLATED]` blocks per request (`:84-88`). |

## 2. The HTTP surface

### 2a. Inbound — the request the game must send (inferred from the parser)

| Element | Value | Evidence |
|---|---|---|
| Scheme / host / port | `http` `127.0.0.1` `5000` | `:13`, `:18` |
| Verb | Not constrained — `HttpMethod` is never read | `:26`, `:30` |
| Path | Ignored. Only `RawUrl` after `?` is used, so any path under the prefix works | `:30-32` |
| Query params | `text`, `from`, `to` | `:34-36` |
| `from` default | `zh-CN` | `:35` |
| `to` default | `en` | `:36` |
| `text` encoding | Percent-encoded UTF-8, decoded by `ParseQueryString`; the raw string is deliberately used "to handle UTF-8 Chinese strings correctly" | `:29-32` |
| Placeholder filter | `text` equal to the literal `{text}`, or whitespace-only, is not translated | `:39` |
| Batching | One Translation Unit per inbound request; the script translates it line-by-line internally | `:46-71` |

### 2b. Outbound — the request the script makes

Literal construction, `translation_proxy.ps1:53-54`:

```powershell
$encodedText = [System.Web.HttpUtility]::UrlEncode($line)
$googleUrl = "https://translate.googleapis.com/translate_a/single?client=gtx&sl=$from&tl=$to&dt=t&q=$encodedText"
```

| Element | Value | Evidence |
|---|---|---|
| Verb | `GET` | `:57` |
| URL | `https://translate.googleapis.com/translate_a/single` | `:54` |
| Params (literal order) | `client=gtx`, `sl=$from`, `tl=$to`, `dt=t`, `q=$encodedText` | `:54` |
| `q` encoding | `HttpUtility.UrlEncode` — lowercase `%xx` UTF-8 hex, space → `+` (verified by running the same call) | `:53` |
| Header | Only `-UserAgent "Mozilla/5.0"` | `:57` |
| Auth / API key | None | `:54-57` |
| TLS | Forced to TLS 1.2 globally | `:21` |
| Timeout | None set by the script; the cmdlet default applies (secondary: MS `Invoke-RestMethod` docs) | `:57` |
| Retries | None — one attempt per line | `:57` |
| Granularity | **One outbound request per non-blank line**, i.e. per physical line of the Translation Unit, not per character and not batched | `:44-64` |
| Blank lines | Whitespace-only lines are passed through untranslated, no call made | `:50-51` |
| Line separators | Not sent to Google; kept aside and rejoined verbatim | `:44`, `:66-69` |

## 3. The response contract

### 3a. What the script does with Google's reply

`Invoke-RestMethod` **deserialises JSON**; the script reads the body structurally, not as raw text (`:57-62`):

```powershell
$apiResponse = Invoke-RestMethod -Uri $googleUrl -Method Get -UserAgent "Mozilla/5.0"
$translatedLine = ""
foreach ($sentence in $apiResponse[0]) {
    if ($sentence[0]) { $translatedLine += $sentence[0] }
}
```

- Content type / charset to expect: **not asserted** by the script; no `-ContentType`/`-Headers`, no charset check (`:57`).
- It reads `$apiResponse[0]` (sentence array) and concatenates `$sentence[0]` of each element (`:60-62`).
- Empty/falsy result → falls back to the original line (`:64`).
- Non-200 from Google: no status handling — `Invoke-RestMethod` raises, the outer `catch` sets **500** and closes (`:97-102`).
- Network failure / slow response: same path — **500 to the game, no Passthrough Response body** (`:97-102`). The "return original on failure" logic exists only for *empty translation results* (`:64`) and *filtered placeholders* (`:89-94`).

### 3b. What it returns to the caller

| Element | Value | Evidence |
|---|---|---|
| Content type | `text/plain; charset=utf-8` | `:76` |
| Length | `ContentLength64` set explicitly | `:77` |
| Body | Raw UTF-8 bytes of the translated string, **not** JSON | `:71`, `:75`, `:78` |
| Line breaks | Original `\r\n`, `\r`, or `\n` separators reinserted unchanged | `:66-69`, `:71` |
| Non-200 to caller | Only `500`, emitted from `catch`, with no body written | `:99-101` |
| Placeholder / blank `text` | Returns `$text` unchanged, UTF-8, 200, content type unset | `:89-94` |
| Where the text goes | Response stream (`:78`) and console only (`:84-88`); nowhere else | `:78`, `:84-88` |

## 4. Verbatim request samples

**Outbound — exactly what the script builds** for a line, with the one literal from the script (`{text}`, `:39`) and the defaults (`:35-36`):

```
GET https://translate.googleapis.com/translate_a/single?client=gtx&sl=zh-CN&tl=en&dt=t&q=%7Btext%7D
User-Agent: Mozilla/5.0
```

`<sl>` and `<tl>` are substituted verbatim from the inbound `from`/`to` (`:35-36`, `:54`); `<q>` is `UrlEncode(<line>)` (`:53`).

**Inbound — reconstructed** from the parser; path is arbitrary because it is discarded (`:30-32`):

```
GET http://127.0.0.1:5000/?text=<urlencoded Translation Unit>&from=zh-CN&to=en
```

**Realistic Translation Unit:** the script reveals that text is Chinese (`zh-CN`, `:35`) but contains **no game dialogue string** to quote — only the literal `{text}` (`:39`). Any concrete sentence would be invented, so none is given. Mechanically, a line `你好，旅行者。` would encode as `%e4%bd%a0%e5%a5%bd%ef%bc%8c%e6%97%85%e8%a1%8c%e8%80%85%e3%80%82` under `:53` (value produced by running `UrlEncode`, not taken from the file).

## 5. Rendering Tokens and escaped line breaks

| Question | Answer | Evidence |
|---|---|---|
| What line breaks exist? | `\r\n`, `\r`, `\n`, matched as `"(\r\n|\r|\n)"` and preserved exactly, "keep it untouched!" | `:44`, `:67-69` |
| Escaped-break syntax? | None. No literal `\n`/`<br>` token handling — only real CR/LF characters | `:44`, `:66-69` |
| Colour / layout markup (`<color=...>`, TMPro tags)? | **Not revealed by this script.** No tag parsing, stripping, or escaping exists anywhere | `:1-104` |
| Does any post-processing strip or re-escape markup? | **No** for the HTTP body — the joined string is written as-is | `:71`, `:75`, `:78` |
| Console-only escaping | `$displayOriginal` / `$displayTranslated` replace CRLF/CR/LF with the visible 4-char text `\r\n` / `\r` / `\n` for display, using regex `-replace` (PowerShell escapes with backtick, so the source text `"\r\n"` is the literal backslash form). Display only; does not touch the response | `:81-82`, `:84-88` |

Consequence: since each line is sent to Google with its markup intact (`:53-54`), any Rendering Token inside a
line is exposed to the translation engine. Whether Google preserves such tokens is **not revealed by this
script**, and the script has no guard for it.

## 6. Quirks worth knowing before building the real proxy

| Quirk | Evidence |
|---|---|
| No `?` in `RawUrl` → `IndexOf` returns -1, `Substring(0)` parses the whole URL as the query | `:31` |
| Blank `text` reaches `Encoding.GetBytes($text)` with a null/empty value in the else branch; may throw into the 500 path | `:89-94`, `:97-102` |
| Translation is line-granular, so a sentence wrapped across lines is translated as fragments | `:44-71` |
| No concurrency: `GetContext` is serial, one translation at a time | `:23-25` |
| No timeout means a stalled Google call stalls the whole proxy for the game | `:57` |
