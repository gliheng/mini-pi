# Remote Control API Guide

`mini-pi` can expose its chat sessions over HTTP so you can build a custom frontend (mobile web app, dashboard, automation script, etc.) that talks to the running desktop app. The desktop app starts a local HTTP server and tunnels it to the public internet via `cloudflared`.

This guide explains how to enable remote control and how to use the exposed REST and Server-Sent Events (SSE) APIs from a frontend.

---

## Table of Contents

- [Overview](#overview)
- [Enabling Remote Control](#enabling-remote-control)
- [Authentication](#authentication)
- [Base URL](#base-url)
- [REST Endpoints](#rest-endpoints)
- [SSE Streaming](#sse-streaming)
- [Data Schemas](#data-schemas)
- [Frontend Workflow Example](#frontend-workflow-example)
- [Error Handling](#error-handling)
- [Security Notes](#security-notes)
- [CORS](#cors)

---

## Overview

When remote control is enabled, the desktop app:

1. Starts a local `axum` server on `127.0.0.1:<bind_port>` (default `9876`).
2. Spawns `cloudflared` to create a public Cloudflare Tunnel (quick tunnel by default).
3. Displays the public tunnel URL and a QR code in the user settings panel.
4. Accepts HTTP requests from any client that can reach the tunnel URL.

The API is intentionally small and stateful: it operates on the app’s existing SQLite-backed threads and the single active "target" session. A frontend can list threads, open one, send messages, and subscribe to live updates.

---

## Enabling Remote Control

Remote control is **always disabled on startup** (the app sets `remote_control.enabled = false` on launch and saves it). To turn it on, either:

- Toggle **Remote Control** in the in-app user settings panel, or
- Edit `~/.config/mini-pi/config.json` and set `remote_control.enabled = true` before launching the app.

Example minimal config:

```json
{
  "default_model": "cloudflare-ai-gateway:gpt-4o-mini",
  "remote_control": {
    "enabled": true,
    "bind_port": 9876
  }
}
```

### Configuration fields

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `remote_control.enabled` | boolean | `false` | Whether the remote API server should run. |
| `remote_control.bind_port` | integer | `9876` | Local port for the HTTP server. |
| `remote_control.bearer_token` | string | `null` | Optional local token. **Highly recommended** for quick tunnels. |
| `remote_control.cloudflared.command` | string | `"cloudflared"` | Path or name of the `cloudflared` binary. |
| `remote_control.cloudflared.tunnel_token` | string | `null` | Use a named Cloudflare tunnel instead of a quick tunnel. |
| `remote_control.cloudflared.hostname` | string | `null` | Required when using a named tunnel token. |

### Named tunnels

For a permanent hostname, set `tunnel_token` and `hostname`:

```json
{
  "remote_control": {
    "enabled": true,
    "cloudflared": {
      "tunnel_token": "<your-tunnel-token>",
      "hostname": "mini-pi.example.com"
    }
  }
}
```

When both are set, `cloudflared tunnel run --token <token>` is used and `hostname` is reported as the public URL.

---

## Authentication

If `bearer_token` is set, every request must include:

```http
Authorization: Bearer <bearer_token>
```

Both `Bearer` and `bearer` casing are accepted. The token is compared in constant time. Missing or wrong tokens receive `401 Unauthorized` with:

```http
WWW-Authenticate: Bearer
```

If no token is configured, all requests are allowed. For quick tunnels over the public internet, **always set a bearer token** or front the tunnel with Cloudflare Access.

---

## Base URL

The public URL is returned by `GET /status` in the `tunnel_url` field. It is also shown in the desktop app as a QR code.

```
https://<random>.trycloudflare.com
```

All endpoints below are relative to this base URL.

---

## REST Endpoints

### `GET /status`

Returns the current remote-control status.

**Response `200 OK`**

```json
{
  "enabled": true,
  "status": "running",
  "status_detail": "running",
  "tunnel_url": "https://abc123.trycloudflare.com",
  "target_thread_id": 42
}
```

`status` can be `disabled`, `starting`, `running`, or `error`. When `status` is `error`, `status_detail` is an object like `{"error": "..."}`.

---

### `GET /threads`

Lists all persisted threads.

**Response `200 OK`**

```json
[
  {
    "id": 1,
    "title": "Rust refactor",
    "preview": "Can you refactor this function?",
    "session_file": "session_....jsonl",
    "model": "cloudflare-ai-gateway:gpt-4o-mini",
    "thinking_level": null,
    "pinned": false,
    "metadata": { "workspace_id": 3 },
    "created_at": "2026-06-17 14:30:00",
    "updated_at": "2026-06-17 14:35:00"
  }
]
```

---

### `POST /threads`

Creates a new chat thread and makes it the active target session.

**Request body**

```json
{
  "workspace_id": 3,
  "model_id": "cloudflare-ai-gateway:gpt-4o-mini"
}
```

Both fields are optional. If `workspace_id` is omitted, the first configured workspace is used. If `model_id` is omitted, the default model or no model is selected.

**Response `201 Created`**

```json
{ "thread_id": 42 }
```

---

### `POST /threads/:id/open`

Opens an existing thread as the active target session. Creates a session file if the thread does not have one.

**Response `200 OK`**

```json
{ "thread_id": 42 }
```

---

### `GET /threads/:id/messages`

Returns the messages for a thread. Use `?since_id=<message_id>` to fetch only messages after the given one.

**Response `200 OK`**

```json
[
  {
    "id": "msg-uuid-1",
    "entry_id": "sdk-entry-1",
    "role": "user",
    "parts": [
      { "type": "text", "text": "Hello", "state": "Done" }
    ]
  },
  {
    "id": "msg-uuid-2",
    "entry_id": "sdk-entry-2",
    "role": "assistant",
    "parts": [
      { "type": "text", "text": "Hi!", "state": "Streaming" }
    ]
  }
]
```

---

### `POST /threads/:id/message`

Sends a user message to the thread.

**Request body**

```json
{ "message": "Refactor this function to use Result" }
```

**Response `202 Accepted`**

```json
{
  "message_id": "msg-uuid-3",
  "status": "accepted"
}
```

The assistant response streams through the SSE endpoint.

---

### `POST /threads/:id/abort`

Aborts the current assistant turn / streaming.

**Response `200 OK`**

```json
{ "status": "aborted" }
```

---

### `POST /threads/:id/model`

Changes the model for the thread.

**Request body**

```json
{ "model_id": "cloudflare-ai-gateway:claude-sonnet-4-6" }
```

**Response `200 OK`**

```json
{ "status": "ok" }
```

Known model IDs are defined in `src/config/model_config.rs`. Examples:

- `cloudflare-ai-gateway:gpt-4o-mini`
- `cloudflare-ai-gateway:gpt-5.5`
- `cloudflare-ai-gateway:claude-sonnet-4-6`
- `cloudflare-ai-gateway:claude-opus-4-8`
- `cloudflare-ai-gateway:@cf/moonshotai/kimi-k2.6`
- `deepseek:deepseek-v4-flash`
- `deepseek:deepseek-v4-pro`

---

### `POST /threads/:id/workspace`

Changes the workspace for the thread.

**Request body**

```json
{ "workspace_id": 3 }
```

**Response `200 OK`**

```json
{ "status": "ok" }
```

---

## SSE Streaming

### `GET /threads/:id/stream`

Opens a long-lived Server-Sent Events stream for live thread updates. The connection sends:

1. An initial `update` event with the full thread snapshot.
2. `delta` events whenever messages are added, updated, removed, or the chat state changes.
3. `:ping` comment heartbeats every 5 seconds to keep proxies and browsers alive.

**Request**

```http
GET /threads/42/stream HTTP/1.1
Authorization: Bearer <token>
Accept: text/event-stream
```

**Initial `update` event**

```
event: update
data: {"state":"idle","messages":[{"id":"...",...}]}

```

**`delta` event while streaming**

```
event: delta
data: {"state":"streaming","added_messages":[],"updated_messages":[{"id":"...","parts":[{"type":"text","text":"Hel","state":"Streaming"}]}],"removed_message_ids":[]}

```

**Heartbeat**

```
: ping

```

### Delta fields

| Field | Description |
|-------|-------------|
| `state` | New chat state if it changed; otherwise `null`. |
| `added_messages` | Full message objects that appeared since the last event. |
| `updated_messages` | Full message objects whose content changed (e.g. streaming text). |
| `removed_message_ids` | IDs of messages that were removed. |

### Consuming SSE from JavaScript

```javascript
const evtSource = new EventSource(`/threads/${threadId}/stream`);

evtSource.addEventListener('update', (e) => {
  const snapshot = JSON.parse(e.data);
  renderMessages(snapshot.messages);
});

evtSource.addEventListener('delta', (e) => {
  const delta = JSON.parse(e.data);
  if (delta.state) updateState(delta.state);
  delta.added_messages.forEach(addMessage);
  delta.updated_messages.forEach(updateMessage);
  delta.removed_message_ids.forEach(removeMessage);
});
```

> **Note:** `EventSource` does not support custom headers such as `Authorization`. If you use a bearer token, either:
> - Put the token in the URL query string as `?access_token=<token>` (also accepted as `?token=<token>`), or
> - Use a library like `eventsource-parser` or manual `fetch` + `ReadableStream` parsing with the header.

---

## Data Schemas

### `Thread`

```json
{
  "id": 42,
  "title": "Thread title",
  "preview": "First few words...",
  "session_file": "session_<uuid>.jsonl",
  "model": "cloudflare-ai-gateway:gpt-4o-mini",
  "thinking_level": null,
  "pinned": false,
  "metadata": {},
  "created_at": "2026-06-17 14:30:00",
  "updated_at": "2026-06-17 14:35:00"
}
```

### `Message`

```json
{
  "id": "<ui-uuid>",
  "entry_id": "<sdk-entry-id>",
  "role": "user" | "assistant",
  "parts": [<Part>]
}
```

### `Part`

A part can be one of:

| `type` | Fields | Description |
|--------|--------|-------------|
| `text` | `text`, `state` | Plain text content. |
| `thinking` | `text`, `state` | Model reasoning / thinking block. |
| `tool_call` | `name`, `args`, `state` | A tool invocation. |
| `tool_result` | `name`, `output`, `state` | Result of a tool call. |

`state` is `"Streaming"` while content is arriving and `"Done"` when finished.

Example text part during streaming:

```json
{
  "type": "text",
  "text": "Here is the refactored",
  "state": "Streaming"
}
```

### `ChatState`

The thread state returned in SSE and snapshot data:

- `"idle"`
- `"loading"`
- `"streaming"`
- `{ "error": "error message" }`

---

## Frontend Workflow Example

A typical web frontend flow:

1. **Read the tunnel URL**
   The user scans the QR code in the desktop app or enters the tunnel URL manually.

2. **List threads**
   ```javascript
   const res = await fetch(`${BASE_URL}/threads`, {
     headers: { Authorization: `Bearer ${TOKEN}` }
   });
   const threads = await res.json();
   ```

3. **Open or create a thread**
   ```javascript
   // Create new
   const res = await fetch(`${BASE_URL}/threads`, {
     method: 'POST',
     headers: {
       'Authorization': `Bearer ${TOKEN}`,
       'Content-Type': 'application/json'
     },
     body: JSON.stringify({ model_id: 'cloudflare-ai-gateway:gpt-4o-mini' })
   });
   const { thread_id } = await res.json();
   ```

4. **Start SSE stream**
   ```javascript
   const stream = new EventSource(
     `${BASE_URL}/threads/${thread_id}/stream?access_token=${TOKEN}`
   );
   stream.addEventListener('update', (e) => renderSnapshot(JSON.parse(e.data)));
   stream.addEventListener('delta', (e) => applyDelta(JSON.parse(e.data)));
   ```

5. **Send a message**
   ```javascript
   await fetch(`${BASE_URL}/threads/${thread_id}/message`, {
     method: 'POST',
     headers: {
       'Authorization': `Bearer ${TOKEN}`,
       'Content-Type': 'application/json'
     },
     body: JSON.stringify({ message: 'Hello!' })
   });
   ```

6. **Render deltas as they arrive**
   The assistant response updates in real time through the SSE stream.

---

## Error Handling

The API uses standard HTTP status codes:

| Status | Meaning |
|--------|---------|
| `200` | Success |
| `201` | Thread created |
| `202` | Message accepted |
| `204` | CORS preflight (`OPTIONS`) |
| `400` | Bad request (invalid thread id, unknown model, malformed body) |
| `401` | Missing or invalid bearer token |
| `404` | Thread or workspace not found, or unknown route |
| `405` | Method not allowed |
| `500` | Server-side error |
| `503` | Remote controller unavailable (SSE only) |

Error bodies follow this shape:

```json
{ "error": "thread not found" }
```

---

## CORS

All responses include:

```http
Access-Control-Allow-Origin: *
```

`OPTIONS` requests receive:

```http
Access-Control-Allow-Methods: GET, POST, OPTIONS
Access-Control-Allow-Headers: Authorization, Content-Type
```

This lets a browser frontend hosted on a different origin talk to the tunnel URL.

---

## Security Notes

- The optional local `bearer_token` is the only authentication for quick tunnels. Treat it like a password.
- For production or long-lived access, use a **named Cloudflare tunnel** with **Cloudflare Access** at the edge instead of relying on the local bearer token.
- All traffic between the frontend and the desktop app transits through Cloudflare’s network. The local server binds only to `127.0.0.1` and is not directly reachable from the LAN.
- Tokens are stored in plaintext in `~/.config/mini-pi/config.json`.

---

## Limitations

- Only one thread is the active "target" at a time. Opening or creating a thread replaces the target session.
- There is no endpoint to list workspaces or models; those must be known by the frontend or hardcoded.
- SSE streams are tied to a single thread; you must open one stream per thread you want to watch.
- Streaming message caches are trimmed after 1000 messages, at which point a full snapshot is sent.
