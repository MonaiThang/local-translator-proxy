# Local Translator Proxy

The shared language for a local game-text translation system that prioritises faithful rendering and low impact on the game machine.

## Translation

**Translation Proxy**:
A local, game-facing service that accepts a Translation Request and returns rendering-safe translated text.
_Avoid_: translator, translation server

**Model Runtime**:
Separately installed local software that hosts and executes the selected translation model.
_Avoid_: proxy, translation service

**Translation Request**:
A game-originated request containing text and a requested source-to-target language pairing.
_Avoid_: sentence, message

**Translation Corpus**:
A curated set of representative game text, rendering syntax, and expected translations used to judge translation quality and fidelity.
_Avoid_: test data, sample file

**Rendering Token**:
A non-linguistic sequence in game text that controls colour or layout and must be returned byte-for-byte in its original order.
_Avoid_: translated markup, formatting symbol

**Translation Unit**:
The complete text handled together in a Translation Request, including its physical line endings and Rendering Tokens.
_Avoid_: line, fragment

**Passthrough Response**:
A game-safe response that reproduces the original Translation Unit unchanged when translation cannot safely be supplied.
_Avoid_: error response, fallback translation
