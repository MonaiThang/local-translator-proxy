# XUnity.AutoTranslator: BepInEx usage, capture, endpoints/proxies (Tale of Wuxia context)

Sources: README.md (bbepis/XUnity.AutoTranslator), CHANGELOG.md, src/Translators/CustomTranslate/CustomTranslateEndpoint.cs, src/XUnity.AutoTranslator.Plugin.Core/Endpoints/Http/HttpEndpoint.cs, local docs (ADR 0002, docs/research/translation-proxy-caller-script.md). Citations are repo paths/URLs/line numbers where applicable.

## 1) How it works with BepInEx (plugin loading, hooks, TextMeshPro/UGUI)
- **BepInEx plugin packages**: README lists `XUnity.AutoTranslator-BepInEx-{VERSION}.zip` installs plugin dlls under `BepInEx/plugins/XUnity.AutoTranslator/` with deps in `BepInEx/core/` (MonoMod.RuntimeDetour/Utils, Mono.Cecil, 0Harmony). IL2CPP uses BepInEx 6 bleeding edge and `BepInEx-IL2CPP` package (README Installation/BepInEx sections).  
- **Core/plugin entry**: `src/XUnity.AutoTranslator.Plugin.BepInEx/AutoTranslatorPlugin.cs` and `src/XUnity.AutoTranslator.Plugin.BepInEx-IL2CPP/AutoTranslatorPlugin.cs` exist; main logic in `src/XUnity.AutoTranslator.Plugin.Core/AutoTranslationPlugin.cs`, `AutoTranslator.cs`, `TranslationManager.cs`, `Hooks/*`.  
- **Hooks/text frameworks**: Supported include UGUI, NGUI, IMGUI (disabled by default), TextMeshPro, TextMesh (disabled), FairyGUI, UIElements, Utage (README Text Frameworks). Hooks implemented in `src/XUnity.AutoTranslator.Plugin.Core/Hooks/{UGUIHooks,TextMeshProHooks,NGUIHooks,IMGUIHooks,UIElementsHooks,FairyGUIHooks,UtageHooks,TextMeshHooks,...}`. Text component manipulation via `Text/ITextComponentManipulator` implementations (e.g. `DefaultTextComponentManipulator`, `UguiNovelTextComponentManipulator`, `FairyGUITextComponentManipulator`).  
- **MonoMod vs Harmony**: MonoMod hooks used if available/cannot hook with Harmony (methods with no body, netstandard2.0 surface); controlled by `ForceMonoModHooks` / `InitializeHarmonyDetourBridge` (README Configuration/MonoMod Hooks). Deps present in BepInEx package.  
- **IL2CPP notes**: subpar hooking/brute force workaround mentioned; some features limited (README IL2CPP Support).

## 2) How it captures text from game UI (translators/scanners/hooks)
- **Text capture**: Hooks intercept text getter/setter paths on supported components (UGUI Text/TMP_Text, NGUI UILabel, TextMesh, IMGUI, UIElements, FairyGUI, Utage). When text changes/appears, plugin evaluates if translatable (language checks, whitespace/dialogue rules, IMGUI templating/numbers handling, blacklist, etc.).  
- **Parsing/preprocessing**: Rich text parsing (`Parsing/RichTextParser.cs`), regex splitting (`RegexSplittingTextParser`, `RegexTranslationSplitter`), game log paths (`GameLogTextPaths`), substitution/preprocessors/postprocessors files, HTML entity preprocessing, templating.  
- **Translation pipeline**: `TranslationManager` queues jobs; spam prevention (delays, caps, connection reuse, shutdown conditions) as documented (README Spam Prevention). Manual hook via `ALT+U`, reload via `ALT+R`.  
- **Caching**: in-memory + disk translation caches (`TextTranslationCache`, `CompositeTextTranslationCache`, static translations). Output to `OutputFile`/translation dirs.

## 3) How it sends requests to translation endpoints/proxies — protocol, method, path, query params, headers, batching, line breaks
- **Endpoint model**: Translators implement `ITranslateEndpoint` (`src/XUnity.AutoTranslator.Plugin.Core/Endpoints/ITranslateEndpoint.cs`); HTTP-based extend `HttpEndpoint` (`Endpoints/Http/HttpEndpoint.cs:11-106`). Flow: `OnBeforeTranslate` (optional coroutine) → `OnCreateRequest` sets `XUnityWebRequest` → `XUnityWebClient.Send()` executes → wait for response coroutine → `OnInspectResponse` → check errors/data → `OnExtractTranslation` completes context.  
- **Method/URL/headers**: Determined by each endpoint implementation. `HttpEndpoint` itself is abstract; concrete endpoints build URLs/requests. `XUnityWebClient/XUnityWebRequest` handle transport (timeouts/connections per implementation).  
- **Batching**: `HttpEndpoint.MaxTranslationsPerRequest` defaults to 1 (can be overridden). `EnableBatching` config enables batching where supported endpoints implement it (README Behaviour/Reducing Translation Requests, changelog notes).  
- **Line breaks**: Plugin's whitespace/dialogue handling affects what is sent (README Whitespace Handling). Splitting via regex/splitter supported; `ForceSplitTextAfterCharacters` may reformat translated result. For HTTP GET query params, text is URL-encoded by endpoint implementations (see CustomTranslate).  
- **Example transport**: `XUnityWebClient` is the internal web client (see `src/XUnity.AutoTranslator.Plugin.Core/Web/*`).

## 4) Known config for using a local HTTP proxy
- **CustomTranslate (generic GET)**: README Translators/CustomTranslate states:
  - Endpoint=`CustomTranslate`
  - `[Custom]` section `Url=http://my-custom-translation-service.net/translate`
  - Example request: `GET http://my-custom-translation-service.net/translate?from=ja&to=en&text=こんにちは` (body-only response)
  - Notes: "try HTTP without SSL first" (Unity Mono SSL issues). "This is a developer-centric option..."  
- **Implementation** (`src/Translators/CustomTranslate/CustomTranslateEndpoint.cs:13-64`):
  - Template: `"{0}?from={1}&to={2}&text={3}"` (`ServicePointTemplateUrl`)
  - Reads `Custom/Url`, `Custom/EnableShortDelay`, `Custom/DisableSpamChecks` via `context.GetOrCreateSetting`  
  - Validates URL, calls `context.DisableCertificateChecksFor(uri.Host)`, appends host to friendly name  
  - OnCreateRequest builds `XUnityWebRequest(string.Format(template, _endpoint, context.SourceLanguage, context.DestinationLanguage, Uri.EscapeDataString(context.UntranslatedText)))` and completes  
  - OnExtractTranslation returns `context.Response.Data` as-is  
- **Other proxy/endpoint patterns**: ExtProtocol endpoints exist (`Endpoints/ExtProtocol/*`, `Translators/Http.ExtProtocol/ExtHttpEndpoint.cs`), third-party endpoints (Sugoi, LlmTranslators, AutoLLMTranslator, AutoPollinationTranslator) — some may use different protocols/configs. For loopback local proxy matching MVP contract, use CustomTranslate with `Url=http://127.0.0.1:5000/` (or any path). ADR 0002 confirms proxy listens on `127.0.0.1:5000` and retains MVP `GET` query params + raw UTF-8 plain-text response.

## 5) Rendering/markup preservation and untranslated/failures
- **Rich text**: `HandleRichText=True` by default enables automated handling of rich text; `PersistRichTextMode` controls how parsed rich text persisted (`Fragment` or `Final`). Rich text parser exists (`Parsing/RichTextParser.cs`). README Behaviour/Text post/pre-processing and Rich Text sections.  
- **Markup exposure**: As observed with the local PowerShell proxy (which forwards lines to Google unchanged), tokens/tags inside text are sent as-is unless stripped by preprocessing; whether translation engines preserve them is not guaranteed by plugin — plugin focuses on sending/receiving text and applying postprocessing.  
- **Untranslated/failures**: `OutputUntranslatableText` controls dumping untranslatable texts to OutputFile. On HTTP errors/timeouts, `HttpEndpoint` fails context with messages ("Timeout", "Nothing was returned", wrapped error) — endpoint behavior varies; spam checks may shut down plugin on consecutive failures (README Spam Prevention). Placeholders/filtered texts may be passed through. Passthrough endpoint exists (`Endpoints/PassthroughTranslateEndpoint.cs`).

## 6) Concrete example request to http://127.0.0.1:5000 (inferred)
Based on CustomTranslate implementation + local proxy contract:

- **Config (BepInEx/config/XUnity.AutoTranslator.cfg or equivalent)**:
```ini
[Service]
Endpoint=CustomTranslate

[Custom]
Url=http://127.0.0.1:5000/translate
```
(or just `http://127.0.0.1:5000/` since path ignored)

- **What XUnity.AutoTranslator sends (GET)**:
```
GET http://127.0.0.1:5000/translate?from=ja&to=en&text=%E3%81%93%E3%82%93%E3%81%AB%E3%81%A1%E3%81%AF
```
- Query params: `from` = source language (e.g. `ja` or from `FromLanguage`), `to` = destination (e.g. `en` from `Language`), `text` = URL-encoded untranslated text via `Uri.EscapeDataString` (CustomTranslate).  
- Method: GET. Headers: set by `XUnityWebClient`/endpoint (not specified in CustomTranslate; may include defaults).  
- Body: none.  
- Response expected: raw UTF-8 plain text (only the translated string) with status 200 (per ADR 0002 / CustomTranslate example). Errors may return non-200 per endpoint handling.

**Tale of Wuxia context**: Not explicitly mentioned in README/changelog/source tree reviewed. If present, it would be covered by general UGUI/TextMeshPro/IMGUI hooks depending on UI framework used by that game (enable relevant text frameworks in config if needed).

**Notes**: Some endpoints use POST/JSON or different params; CustomTranslate is the direct match for a simple loopback proxy. Batching not used by CustomTranslate (MaxTranslationsPerRequest defaults to 1). Line breaks in source text are passed as characters in the encoded `text` value (encoding/handling follows endpoint). 
