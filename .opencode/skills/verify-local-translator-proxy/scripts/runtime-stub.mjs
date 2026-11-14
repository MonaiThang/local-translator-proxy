// Deterministic stand-in for the Model Runtime (Ollama's /api/chat).
//
// The Translation Proxy talks to a separately installed Model Runtime. That
// boundary is where a real external system lives, so verification replaces it
// with this stub and drives the real proxy binary over HTTP.
//
// Environment:
//   STUB_PORT      loopback port to listen on (default 7331)
//   STUB_MODE      translate | echo | mismatch | inject | fail (default translate)
//   STUB_DELAY_MS  delay before answering, for deadline tests (default 0)
//
// Control endpoints:
//   GET  /health    { ok, mode, requests }
//   GET  /requests  every received model call, with its source line
//   POST /reset     clear the request log
//
// Every request is logged to stdout as a JSON line so the run's evidence shows
// exactly which lines reached the model.

import http from "node:http";

const port = Number(process.env.STUB_PORT ?? 7331);
const mode = process.env.STUB_MODE ?? "translate";
const delayMs = Number(process.env.STUB_DELAY_MS ?? 0);

/** @type {{ at: string, line: string }[]} */
let requests = [];

function sendJson(response, status, payload) {
  const body = JSON.stringify(payload);
  response.writeHead(status, {
    "content-type": "application/json",
    "content-length": Buffer.byteLength(body),
  });
  response.end(body);
}

function translationFor(line) {
  switch (mode) {
    case "echo":
      return line;
    case "mismatch":
      // Drops every angle-bracket and NGUI bracket token, so any tokenized
      // line fails positional verification.
      return line.replace(/<[^>]*>/g, "").replace(/\[[0-9A-Fa-f]{6}\]/g, "").replace(/\[-\]/g, "");
    case "inject":
      // A real newline in the translation must be rejected.
      return `EN:${line}\n`;
    case "translate":
    default:
      // Prefixing keeps every Rendering Token in place, so the proxy's
      // positional verification passes.
      return `EN:${line}`;
  }
}

const server = http.createServer(async (request, response) => {
  const url = request.url ?? "/";

  if (request.method === "GET" && url === "/health") {
    return sendJson(response, 200, { ok: true, mode, requests: requests.length });
  }
  if (request.method === "GET" && url === "/requests") {
    return sendJson(response, 200, requests);
  }
  if (request.method === "POST" && url === "/reset") {
    requests = [];
    return sendJson(response, 200, { ok: true });
  }
  if (request.method !== "POST" || url !== "/api/chat") {
    return sendJson(response, 404, { error: "not found" });
  }

  let raw = "";
  for await (const chunk of request) raw += chunk;

  let body;
  try {
    body = JSON.parse(raw);
  } catch {
    return sendJson(response, 400, { error: "request body was not JSON" });
  }

  const line = body?.messages?.[1]?.content ?? "";
  requests.push({ at: new Date().toISOString(), line });
  console.log(`[stub] ${JSON.stringify({ mode, line })}`);

  if (delayMs > 0) await new Promise((resolve) => setTimeout(resolve, delayMs));
  if (mode === "fail") return sendJson(response, 500, { error: "stub failure" });

  sendJson(response, 200, {
    message: { role: "assistant", content: JSON.stringify({ translation: translationFor(line) }) },
  });
});

server.listen(port, "127.0.0.1", () => {
  console.log(`[stub] listening on http://127.0.0.1:${port} mode=${mode} delayMs=${delayMs}`);
});

for (const signal of ["SIGINT", "SIGTERM"]) {
  process.on(signal, () => server.close(() => process.exit(0)));
}
