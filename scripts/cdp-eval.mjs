// Evaluate a JS expression inside the Thuki WebView via CDP.
// Usage: node scripts/cdp-eval.mjs "<expression>" [--await]
// Needs the app started with WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9222
const expr = process.argv[2];
const doAwait = process.argv.includes("--await");

const list = await (await fetch("http://127.0.0.1:9222/json")).json();
const page = list.find((t) => t.type === "page" && t.url.includes("localhost:1420"));
if (!page) {
  console.error("NO PAGE TARGET. Targets:", list.map((t) => `${t.type}:${t.url}`).join(", "));
  process.exit(1);
}

const ws = new WebSocket(page.webSocketDebuggerUrl);
let nextId = 1;
const pending = new Map();

function send(method, params) {
  return new Promise((resolve, reject) => {
    const id = nextId++;
    pending.set(id, { resolve, reject });
    ws.send(JSON.stringify({ id, method, params }));
  });
}

ws.addEventListener("message", (ev) => {
  const msg = JSON.parse(ev.data);
  if (msg.id && pending.has(msg.id)) {
    const { resolve, reject } = pending.get(msg.id);
    pending.delete(msg.id);
    if (msg.error) reject(new Error(JSON.stringify(msg.error)));
    else resolve(msg.result);
  }
});

await new Promise((resolve, reject) => {
  ws.addEventListener("open", resolve);
  ws.addEventListener("error", reject);
});

try {
  const out = await send("Runtime.evaluate", {
    expression: expr,
    awaitPromise: !!doAwait,
    returnByValue: true,
  });
  if (out.exceptionDetails) {
    console.error("PAGE EXCEPTION:", JSON.stringify(out.exceptionDetails, null, 2));
    process.exitCode = 2;
  } else {
    console.log(JSON.stringify(out.result?.value ?? out.result, null, 2));
  }
} finally {
  ws.close();
}
