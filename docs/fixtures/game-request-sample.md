# Game request sample (from translation_proxy.ps1)

Source: [translation_proxy.ps1](https://github.com/MonaiThang/local-translator-proxy/blob/main/docs/research/translation-proxy-caller-script.md) (the live script is a proxy to Google; this documents what the game calls and what it expects back)

## Inbound (game → local proxy)

- **Method**: GET (not validated)
- **Scheme/Host/Port**: `http://127.0.0.1:5000`
- **Path**: any path under `/` is accepted; only query string is parsed
- **Query parameters**:
  - `text` (required): percent-encoded UTF-8 of the Translation Unit
  - `from` (optional): source language, default `zh-CN`
  - `to` (optional): target language, default `en`
- **Encoding**: `System.Web.HttpUtility` parses `RawUrl`; text is percent-encoded UTF-8
- **Placeholder**: literal `{text}` or whitespace-only → returned unchanged (200)

### Example URL (reconstructed)
```http
GET http://127.0.0.1:5000/?text=%e4%bd%a0%e5%a5%bd%ef%bc%8c%e6%97%85%e8%a1%8c%e8%80%85%e3%80%82&from=zh-CN&to=en
```
(Note: concrete sentence is illustrative; script reveals Chinese as default but no specific game dialogue.)

## Translation Unit granularity
- Split on `(\r\n|\r|\n)` into parts; even indices are text, odd are separators
- **One outbound request per non-blank line** to Google; blank lines pass through untranslated
- Separators reinserted verbatim into the response

## Response (local proxy → game)
- **Status**: 200 on success/placeholder/blank; **500** on network/Google failure (no body)
- **Content-Type**: `text/plain; charset=utf-8`
- **Body**: raw UTF-8 bytes of the joined translated string (not JSON). Line breaks match original.
- **Error handling**: no Passthrough on translate failure — game gets 500 with empty body

## Rendering Tokens
- **Line breaks**: real CR/LF only. No `<br>`/escaped `\n` token parsing revealed.
- **Markup**: no `<color=...>` or TMPro tag handling revealed; tags are sent to Google as-is with no preservation/verification by the script. Display uses escaped forms only for console logging.
