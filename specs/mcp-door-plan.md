# Door 2 — Thuki as an MCP Server (plan by Claude, 2026-09-17)

> Companion to specs/assistant-roadmap-v2.md. The owner's MCP idea:
> outside AIs (ChatGPT subscription, Claude Desktop, ZCode) connect to
> Thuki and use its tools on the laptop. Adds phases 2.3b (stdio) and
> 2.3c (HTTP+tunnel) after Phase 2.3. Awaiting owner approval.

# Door 2: Thuki as an MCP Server

## Simple English — for you

### What this actually means

Right now Thuki has one brain talking to its tools: the built-in chat (Door 1). Door 2 opens the same tools to any outside AI — ChatGPT with your subscription, Claude Desktop, ZCode, Cursor — so you can say "take a screenshot" inside ChatGPT and it fires Thuki's `capture_screen` on your laptop. The tools are written once in Rust; both doors use them.

---

### The two kinds of connection ("transports")

**Kind 1 — stdio (for Claude Desktop and ZCode)**
The AI app launches a small Thuki helper program (`thuki-mcp.exe`) in the background. They talk through the program's input/output pipes. No network port needed. Claude Desktop and ZCode both work this way.

**Kind 2 — HTTP (for ChatGPT)**
Thuki runs a tiny web server on your machine (`http://localhost:PORT/mcp`). But here is the key problem: ChatGPT lives in OpenAI's cloud — it cannot reach your `localhost`. So you need a free tunnel (Cloudflare Tunnel, one command, free forever) that gives your machine a public address like `https://abc123.trycloudflare.com/mcp`. You paste that URL into ChatGPT settings. The tunnel forwards ChatGPT's requests to Thuki on your laptop.

---

### Safety

Every action tool (click, type, launch app) still pops the yellow YES/NO confirm card on your screen, no matter whether the request came from the built-in chat or from ChatGPT. The confirm gate lives in Rust and cannot be bypassed.

Extra protections for the HTTP door:
- The server only listens on `127.0.0.1` (your machine only — the tunnel is the only way in)
- A secret token (like a password) is required — you set it once in Thuki's settings
- Without the token, the connection is rejected

---

### What you do on the client side

**Claude Desktop:**
Open the file `%AppData%\Claude\claude_desktop_config.json`, add a few lines pointing to `thuki-mcp.exe`. Restart Claude Desktop. Done — Thuki's tools appear in Claude's tool panel.

**ZCode:**
Open Zed's `settings.json` (Settings → Open Settings File), add a `context_servers` block pointing to `thuki-mcp.exe`. Restart Zed.

**ChatGPT (subscription required):**
1. Start the Cloudflare Tunnel on your machine (one command)
2. In ChatGPT → Settings → Connectors → Add → paste the tunnel URL
3. ChatGPT can now call Thuki's tools

---

### Where this fits in the roadmap

| Phase | What it adds | Prerequisite |
|---|---|---|
| **2.3** (agent loop) | Thuki's own AI can call tools | — |
| **Door 2 — read-only** | Claude Desktop + ZCode + ChatGPT can call read-only tools (screenshot, windows list, clipboard, time) | Phase 2.3 tools must exist |
| **2.4 / 2.5** | Voice | — |
| **2.6** (action tools) | Click, type, launch — in both the built-in chat AND over MCP | Door 2 HTTP server already running |

**My recommendation:** build Door 2 (read-only, stdio first) immediately after Phase 2.3. It is roughly 1 week of extra work on top of 2.3. You get GPT-driven screenshot + clipboard read early, which is a big motivator. HTTP (ChatGPT) adds another 3–5 days.

---

### Risks to know

| Risk | Reality |
|---|---|
| ChatGPT needs a public URL | True. The free Cloudflare Tunnel solves it |
| ChatGPT needs OAuth for published plugins | Only for apps you publish publicly. For your own personal use, a simple secret token is fine |
| Image file size | A full 1080p screenshot is ~350 KB over the wire. Fine for stdio clients; we'll add an optional scale-down for ChatGPT if needed |
| Action tools (click/type) from outside AI | Safe: the confirm card always fires on your screen first |

---

## Technical Notes — for me (Claude)

### Architecture

```
src-tauri/src/          ← shared tool implementations (Rust functions)
    tools/
        capture.rs      ← already exists as capture.rs
        windows.rs      ← list_windows, get_active_window (Phase 2.3)
        clipboard.rs    ← already exists
        datetime.rs     ← get_datetime (Phase 2.3)
        speak.rs        ← SAPI (Phase 2.3)

src-tauri/src/mcp/
    server.rs           ← rmcp ServerHandler impl, tool routing
    http.rs             ← axum + StreamableHttpService (ChatGPT)
    auth.rs             ← bearer token middleware

src-tauri-mcp/          ← separate Cargo binary crate for stdio transport
    main.rs             ← runs rmcp stdio(), imports tools from shared lib
```

The tools lib becomes a workspace crate shared between the Tauri app binary and the `thuki-mcp` stdio binary. The Tauri app spawns the HTTP MCP server in a `tokio::spawn` on startup.

### Crates to add

```toml
# in src-tauri/Cargo.toml
rmcp = { version = "3", features = [
    "server", "macros", "schemars",
    "transport-io",                        # stdio
    "transport-streamable-http-server",    # HTTP (ChatGPT)
    "reqwest",
] }
rmcp-macros = "3"
schemars = "1"
axum = "0.8"

# reqwest must be bumped from 0.12 → 0.13 (rmcp 3.x requires it)
reqwest = { version = "0.13", features = ["rustls-tls", "json", "stream"] }
```

### Transport choice & why

- **stdio first**: zero networking, works immediately with Claude Desktop and ZCode, no tunnel needed. Separate `thuki-mcp.exe` binary using `rmcp`'s `transport-io` feature.
- **HTTP second**: uses `axum` + `rmcp`'s `StreamableHttpService`. Bind to `127.0.0.1:7343` (arbitrary port, configurable). Add bearer token middleware. The Tauri process spawns this server; it does not block the GUI because it runs in `tokio::spawn`.
- **Old SSE transport (deprecated)**: do not implement.

### Image return

```rust
ContentBlock::image(base64_png_string, "image/png")
// → { "type": "image", "data": "<b64>", "mimeType": "image/png" }
```

The existing `capture.rs` already returns `base64_png`. Wire it directly into `ContentBlock::image`. Add optional `scale: Option<f32>` parameter to the MCP tool (default `1.0`; can pass `0.5` to halve dimensions for HTTP clients).

### Safety for HTTP transport

- Bind to `127.0.0.1` only (DNS rebinding protection — also validate `Origin` header per MCP spec)
- Bearer token in `Authorization: Bearer <token>` header; token stored in Windows Credential Manager (same as API keys)
- Action tools (Phase 2.6) emit a Tauri event to the frontend → confirm card → `AppHandle` channel resolves → tool either executes or returns `"user declined"`; this path is identical regardless of whether the caller is Door 1 or Door 2

### Phase ordering (revised roadmap)

```
2.3  Agent loop + 6 read-only tools    ~2 weeks
2.3b Door 2 stdio (Claude Desktop, ZCode)  ~1 week   ← NEW, immediately after 2.3
2.3c Door 2 HTTP (ChatGPT)             ~3–5 days   ← NEW, immediately after 2.3b
2.4  Voice push-to-talk                ~1 week
2.5  Eyes + voice capture-on-speak     ~3–4 days
2.6  Action tools (click, type, etc.)  ~3 weeks    ← actions available over BOTH doors
2.7  Wake word                         deferred
```

---

**Your call:** do you approve this plan? If yes, I'll write the Phase 2.3 spec (agent loop) first — then immediately follow with the Door 2 spec (MCP server, phases 2.3b and 2.3c).
