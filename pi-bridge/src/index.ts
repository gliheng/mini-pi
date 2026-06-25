import { WebSocketServer, WebSocket } from "ws";
import * as fs from "node:fs";
import { randomUUID } from "node:crypto";
import {
  type CreateAgentSessionRuntimeFactory,
  type ToolDefinition,
  type ExtensionUIContext,
  createAgentSessionFromServices,
  createAgentSessionRuntime,
  createAgentSessionServices,
  getAgentDir,
  SessionManager,
  AuthStorage,
  ModelRegistry,
  DefaultResourceLoader,
  AgentSessionRuntime,
} from "@earendil-works/pi-coding-agent";
import { createSendFileTool } from "./send-file-tool.js";


const authStorage = AuthStorage.create();
const modelRegistry = ModelRegistry.create(authStorage);

interface SessionState {
  runtime: AgentSessionRuntime;
  sessionManager: any;
  unsubscribe: () => void;
  cwd: string;
  agentDir: string;
}

const sessions = new Map<string, SessionState>();
const pendingExtensionRequests = new Map<
  string,
  { resolve: (response: any) => void; reject: (error: any) => void }
>();
let wss: WebSocketServer | null = null;
let clientSocket: WebSocket | null = null;

function log(...args: unknown[]) {
  console.error("[pi-bridge]", ...args);
}

function send(ws: WebSocket, message: unknown) {
  if (ws.readyState === WebSocket.OPEN) {
    ws.send(JSON.stringify(message));
  }
}

function sendResponse(
  ws: WebSocket,
  sessionId: string,
  command: string,
  requestId: string | undefined,
  success: boolean,
  data?: unknown,
  error?: string
) {
  send(ws, {
    type: "response",
    sessionId,
    command,
    id: requestId,
    success,
    data,
    error,
  });
}

// ---------------------------------------------------------------------------
// Custom tools
// ---------------------------------------------------------------------------

const sendFileTool = createSendFileTool();

function createRuntimeFactory(
  cwd: string,
  customTools: ToolDefinition[] = []
): CreateAgentSessionRuntimeFactory {
  return async ({ cwd: factoryCwd, sessionManager, sessionStartEvent }) => {
    const effectiveCwd = factoryCwd || cwd;
    const services = await createAgentSessionServices({ cwd: effectiveCwd });
    return {
      ...(await createAgentSessionFromServices({
        services,
        sessionManager,
        sessionStartEvent,
        customTools,
      })),
      services,
      diagnostics: services.diagnostics,
    };
  };
}

function resolveModel(provider: string, modelId: string): ReturnType<typeof modelRegistry.find> {
  return modelRegistry.find(provider, modelId);
}

function forwardEvent(ws: WebSocket, sessionId: string, event: any) {
  send(ws, { sessionId, ...event });
}

function subscribeToSession(ws: WebSocket, sessionId: string, runtime: any): () => void {
  const unsubscribe = runtime.session.subscribe((event: any) => {
    forwardEvent(ws, sessionId, event);
  });
  return unsubscribe;
}

// ---------------------------------------------------------------------------
// Extension UI Context — bridges ctx.ui.* calls to the Rust GUI over WebSocket
// ---------------------------------------------------------------------------

function createBridgeUIContext(
  sessionId: string,
  ws: WebSocket
): ExtensionUIContext {
  function emitFireAndForget(
    method: string,
    fields: Record<string, any>
  ): void {
    const id = randomUUID();
    send(ws, { type: "extension_ui_request", sessionId, id, method, ...fields });
  }

  function createDialog<T>(
    method: string,
    defaultValue: T,
    fields: Record<string, any>,
    opts: { signal?: AbortSignal; timeout?: number } | undefined,
    extractResult: (response: any) => T
  ): Promise<T> {
    if (opts?.signal?.aborted) return Promise.resolve(defaultValue);

    const id = randomUUID();
    return new Promise<T>((resolve, reject) => {
      let timer: ReturnType<typeof setTimeout> | undefined;

      const done = (value: T) => {
        if (timer) clearTimeout(timer);
        pendingExtensionRequests.delete(id);
        resolve(value);
      };

      if (opts?.timeout) {
        timer = setTimeout(() => done(defaultValue), opts.timeout);
      }

      if (opts?.signal) {
        opts.signal.addEventListener("abort", () => done(defaultValue), {
          once: true,
        });
      }

      pendingExtensionRequests.set(id, {
        resolve: (response: any) => done(extractResult(response)),
        reject: (err: any) => {
          if (timer) clearTimeout(timer);
          pendingExtensionRequests.delete(id);
          reject(err);
        },
      });

      const wireFields: Record<string, any> = { ...fields };
      if (opts?.timeout) wireFields.timeout = opts.timeout;
      send(ws, {
        type: "extension_ui_request",
        sessionId,
        id,
        method,
        ...wireFields,
      });
    });
  }

  return {
    select: (title, options, opts) =>
      createDialog("select", undefined, { title, options }, opts, (r) =>
        r.cancelled ? undefined : r.value
      ),
    confirm: (title, message, opts) =>
      createDialog("confirm", false, { title, message }, opts, (r) =>
        r.cancelled ? false : r.confirmed === true
      ),
    input: (title, placeholder, opts) =>
      createDialog("input", undefined, { title, placeholder }, opts, (r) =>
        r.cancelled ? undefined : r.value
      ),
    editor: (title, prefill) =>
      createDialog("editor", undefined, { title, prefill }, undefined, (r) =>
        r.cancelled ? undefined : r.value
      ),

    notify(message, type) {
      emitFireAndForget("notify", { message, notifyType: type });
    },
    setStatus(key, text) {
      emitFireAndForget("setStatus", { statusKey: key, statusText: text });
    },
    setWidget(key, content, options) {
      if (content === undefined || Array.isArray(content)) {
        emitFireAndForget("setWidget", {
          widgetKey: key,
          widgetLines: content,
          widgetPlacement: options?.placement,
        });
      }
    },
    setTitle(title) {
      emitFireAndForget("setTitle", { title });
    },
    setEditorText(text) {
      emitFireAndForget("set_editor_text", { text });
    },

    // TUI-only / unsupported — no-ops matching the SDK's noOpUIContext
    onTerminalInput: () => () => {},
    setWorkingMessage: () => {},
    setWorkingVisible: () => {},
    setWorkingIndicator: () => {},
    setHiddenThinkingLabel: () => {},
    setFooter: () => {},
    setHeader: () => {},
    custom: async () => undefined as any,
    pasteToEditor: (text) => emitFireAndForget("set_editor_text", { text }),
    getEditorText: () => "",
    addAutocompleteProvider: () => {},
    setEditorComponent: () => {},
    getEditorComponent: () => undefined,
    get theme() {
      return {} as any;
    },
    getAllThemes: () => [],
    getTheme: () => undefined,
    setTheme: () => ({
      success: false,
      error: "Theme switching not supported in bridge mode",
    }),
    getToolsExpanded: () => false,
    setToolsExpanded: () => {},
  };
}

async function rebindExtensions(
  ws: WebSocket,
  sessionId: string,
  state: SessionState
): Promise<void> {
  await state.runtime.session.bindExtensions({
    uiContext: createBridgeUIContext(sessionId, ws),
    mode: "rpc",
    abortHandler: () => {
      try {
        state.runtime.session.abort();
      } catch (e) {
        log("extension abortHandler failed:", e);
      }
    },
    shutdownHandler: () => {
      log("extension requested shutdown for", sessionId);
    },
    onError: (err: any) => {
      forwardEvent(ws, sessionId, {
        type: "extension_error",
        extensionPath: err.extensionPath,
        event: err.event,
        error: err.error,
      });
    },
  });
}

async function handleCreateSession(
  ws: WebSocket,
  msg: any
): Promise<void> {
  const sessionId = msg.sessionId;
  if (!sessionId) {
    send(ws, { type: "error", error: "missing sessionId" });
    return;
  }

  log("create_session:", sessionId, msg.sessionPath, msg.cwd, msg.model);

  const existing = sessions.get(sessionId);
  if (existing) {
    // Session already exists; keep it running and just acknowledge the request.
    // This makes create_session idempotent on the Rust side.
    sendResponse(ws, sessionId, "create_session", msg.id, true, {
      sessionId,
      sessionFile: existing.runtime.session.sessionFile,
    });
    return;
  }

  const cwd = msg.cwd || process.cwd();
  const agentDir = msg.agentDir || getAgentDir();
  const sessionPath: string | undefined = msg.sessionPath;

  let sessionManager: any;
  if (sessionPath) {
    try {
      log("opening session:", sessionPath);
      sessionManager = SessionManager.open(sessionPath);
      log("session opened:", sessionPath);
    } catch (e: any) {
      log("failed to open session, creating new:", sessionPath, e?.message || e);
      sessionManager = SessionManager.create(cwd);
    }
  } else if (cwd) {
    log("creating session for cwd:", cwd);
    sessionManager = SessionManager.create(cwd);
  } else {
    log("creating in-memory session");
    sessionManager = SessionManager.inMemory();
  }

  log("creating runtime...");
  const runtime = await createAgentSessionRuntime(
    createRuntimeFactory(cwd, [sendFileTool]),
    {
      cwd,
      agentDir,
      sessionManager,
    }
  );
  log("runtime created, sessionFile:", runtime.session.sessionFile);

  if (msg.model) {
    const [provider, modelId] = String(msg.model).split(":") as [
      string,
      string
    ];
    if (provider && modelId) {
      try {
        const model = resolveModel(provider, modelId);
        if (model) {
          await runtime.session.setModel(model);
        } else {
          log("model not found:", msg.model);
        }
      } catch (e) {
        log("failed to set model:", e);
      }
    }
  }

  if (msg.thinkingLevel) {
    try {
      runtime.session.setThinkingLevel(String(msg.thinkingLevel) as any);
    } catch (e) {
      log("failed to set thinking level:", e);
    }
  }

  const unsubscribe = subscribeToSession(ws, sessionId, runtime);
  const state: SessionState = {
    runtime,
    sessionManager,
    unsubscribe,
    cwd,
    agentDir,
  };
  sessions.set(sessionId, state);

  log("binding extensions for", sessionId);
  await rebindExtensions(ws, sessionId, state);

  sendResponse(ws, sessionId, "create_session", msg.id, true, {
    sessionId,
    sessionFile: runtime.session.sessionFile,
  });
}

async function readSessionJsonl(sessionPath: string): Promise<any[]> {
  const messages: any[] = [];
  if (!fs.existsSync(sessionPath)) {
    return messages;
  }
  const text = await fs.promises.readFile(sessionPath, "utf8");
  for (const line of text.split("\n")) {
    const trimmed = line.trim();
    if (!trimmed) continue;
    try {
      const entry = JSON.parse(trimmed);

      // Newer pi session files wrap every entry in `type: "message"`
      // with the actual message stored in `entry.message`.
      const message = entry.type === "message" ? entry.message : entry;
      if (!message) continue;

      const role = message.role;
      const id = entry.id || message.id;
      const content = message.content;

      if (role === "user") {
        if (typeof content === "string") {
          messages.push({
            id,
            role: "user",
            content: [{ text: content }],
          });
        } else if (Array.isArray(content)) {
          messages.push({ id, role: "user", content });
        }
      } else if (role === "assistant") {
        const parts: any[] = [];
        if (Array.isArray(content)) {
          for (const block of content) {
            if (block.type === "text") {
              parts.push({ type: "text", text: block.text });
            } else if (block.type === "thinking") {
              parts.push({ type: "thinking", thinking: block.thinking });
            } else if (block.type === "toolCall") {
              parts.push({
                type: "toolCall",
                name: block.name,
                arguments: block.arguments,
              });
            }
          }
        } else if (typeof content === "string") {
          parts.push({ type: "text", text: content });
        }
        messages.push({ id, role: "assistant", content: parts });
      } else if (role === "toolResult") {
        messages.push({
          id,
          role: "toolResult",
          toolName: message.toolName || message.toolCallId,
          content,
        });
      } else if (entry.type === "bashExecution") {
        // Legacy RPC mode top-level bash execution entry.
        messages.push({
          id: entry.id,
          role: "bashExecution",
          command: entry.command,
          output: entry.output,
          exitCode: entry.exitCode,
        });
      }
    } catch {
      // ignore malformed lines
    }
  }
  return messages;
}

function agentMessageToWire(entryId: string, msg: any): any | null {
  const role = msg.role;
  if (role === "user") {
    const content = msg.content;
    if (typeof content === "string") {
      return { id: entryId, role: "user", content: [{ type: "text", text: content }] };
    }
    if (Array.isArray(content)) {
      return { id: entryId, role: "user", content };
    }
    return { id: entryId, role: "user", content: [] };
  }

  if (role === "assistant") {
    const parts: any[] = [];
    const content = msg.content;
    if (Array.isArray(content)) {
      for (const block of content) {
        if (block.type === "text") {
          parts.push({ type: "text", text: block.text });
        } else if (block.type === "thinking") {
          parts.push({ type: "thinking", thinking: block.thinking });
        } else if (block.type === "toolCall") {
          parts.push({
            type: "toolCall",
            name: block.name,
            arguments: block.arguments,
          });
        }
      }
    }
    return { id: entryId, role: "assistant", content: parts };
  }

  if (role === "toolResult") {
    return {
      id: entryId,
      role: "toolResult",
      toolName: msg.toolName || msg.toolCallId,
      content: msg.content,
    };
  }

  // Bash execution messages are custom types in the SDK; skip unknowns here.
  return null;
}

async function handleGetMessages(
  ws: WebSocket,
  msg: any,
  state: SessionState
): Promise<void> {
  const sessionFile = state.runtime.session.sessionFile;
  log("get_messages:", msg.sessionId, "sessionFile:", sessionFile);
  try {
    let messages: any[] = [];

    // Prefer SessionManager.getBranch() so we only return the current leaf path,
    // matching how the pi TUI renders branched sessions.
    if (state.sessionManager && typeof state.sessionManager.getBranch === "function") {
      const branch = state.sessionManager.getBranch();
      log("sessionManager.getBranch returned", branch.length, "entries");
      for (const entry of branch) {
        if (entry.type === "message" && entry.message) {
          const wired = agentMessageToWire(entry.id, entry.message);
          if (wired) {
            messages.push(wired);
          }
        }
      }
    } else if (sessionFile && fs.existsSync(sessionFile)) {
      messages = await readSessionJsonl(sessionFile);
      log("read", messages.length, "messages from", sessionFile);
    } else {
      messages = state.runtime.session.agent?.state?.messages || [];
      log("fallback to agent state,", messages.length, "messages");
    }

    sendResponse(ws, msg.sessionId, "get_messages", msg.id, true, {
      messages,
    });
  } catch (e: any) {
    log("get_messages failed:", e);
    sendResponse(
      ws,
      msg.sessionId,
      "get_messages",
      msg.id,
      false,
      undefined,
      e?.message || String(e)
    );
  }
}

async function handleGetCommands(
  ws: WebSocket,
  msg: any,
  state: SessionState
): Promise<void> {
  try {
    const loader = new DefaultResourceLoader({
      cwd: state.cwd,
      agentDir: state.agentDir,
    });
    await loader.reload();
    const promptsResult = loader.getPrompts?.() || { prompts: [] };
    const skillsResult = loader.getSkills?.() || { skills: [] };
    const prompts = promptsResult.prompts || [];
    const skills = skillsResult.skills || [];
    const commands: any[] = [];
    for (const prompt of prompts) {
      commands.push({
        name: prompt.name,
        description: (prompt as any).description,
        source: (prompt as any).source || "prompt",
      });
    }
    for (const skill of skills) {
      commands.push({
        name: skill.name,
        description: skill.description,
        source: "skill",
      });
    }
    sendResponse(ws, msg.sessionId, "get_commands", msg.id, true, {
      commands,
    });
  } catch (e: any) {
    sendResponse(
      ws,
      msg.sessionId,
      "get_commands",
      msg.id,
      false,
      undefined,
      e?.message || String(e)
    );
  }
}

async function handleGetModels(ws: WebSocket, msg: any): Promise<void> {
  try {
    const models = await modelRegistry.getAvailable();
    sendResponse(ws, msg.sessionId || "bridge", "get_models", msg.id, true, {
      models: models.map((m: any) => ({
        provider: m.provider,
        id: m.id,
        name: m.name,
        thinkingLevelMap: m.thinkingLevelMap ?? undefined,
      })),
    });
  } catch (e: any) {
    sendResponse(
      ws,
      msg.sessionId || "bridge",
      "get_models",
      msg.id,
      false,
      undefined,
      e?.message || String(e)
    );
  }
}

async function handleGetModel(
  ws: WebSocket,
  msg: any,
  state: SessionState | undefined
): Promise<void> {
  try {
    let model: any;

    if (msg.provider && msg.modelId) {
      model = resolveModel(String(msg.provider), String(msg.modelId));
      if (!model) {
        throw new Error(`model not found: ${msg.provider}:${msg.modelId}`);
      }
    } else if (state?.runtime.session.model) {
      model = state.runtime.session.model;
    } else {
      throw new Error("missing provider/modelId or active session model");
    }

    sendResponse(ws, msg.sessionId || "bridge", "get_model", msg.id, true, {
      model: model
        ? {
            provider: model.provider,
            id: model.id,
            name: model.name,
          }
        : null,
    });
  } catch (e: any) {
    sendResponse(
      ws,
      msg.sessionId || "bridge",
      "get_model",
      msg.id,
      false,
      undefined,
      e?.message || String(e)
    );
  }
}

async function handleSetModel(
  ws: WebSocket,
  msg: any,
  state: SessionState
): Promise<void> {
  try {
    const provider = String(msg.provider);
    const modelId = String(msg.modelId);
    const model = resolveModel(provider, modelId);
    if (!model) {
      throw new Error(`model not found: ${provider}:${modelId}`);
    }
    await state.runtime.session.setModel(model);
    sendResponse(ws, msg.sessionId, "set_model", msg.id, true);
  } catch (e: any) {
    sendResponse(
      ws,
      msg.sessionId,
      "set_model",
      msg.id,
      false,
      undefined,
      e?.message || String(e)
    );
  }
}

async function dispatch(ws: WebSocket, msg: any): Promise<void> {
  const type = msg.type;
  const sessionId = msg.sessionId;

  if (type === "create_session") {
    await handleCreateSession(ws, msg);
    return;
  }

  if (type === "get_models") {
    await handleGetModels(ws, msg);
    return;
  }

  if (type === "get_model") {
    const state = sessionId ? sessions.get(sessionId) : undefined;
    await handleGetModel(ws, msg, state);
    return;
  }

  if (!sessionId) {
    send(ws, { type: "error", error: "missing sessionId" });
    return;
  }

  const state = sessions.get(sessionId);
  if (!state) {
    log("session not found for command:", type, sessionId);
    sendResponse(
      ws,
      sessionId,
      type,
      msg.id,
      false,
      undefined,
      "session not found"
    );
    return;
  }

  const session = state.runtime.session;

  try {
    switch (type) {
      case "prompt": {
        const options: any = {};
        if (msg.images && msg.images.length > 0) {
          options.images = msg.images;
        }
        if (msg.streamingBehavior) {
          options.streamingBehavior = msg.streamingBehavior;
        }
        await session.prompt(String(msg.message), options);
        sendResponse(ws, sessionId, "prompt", msg.id, true);
        break;
      }
      case "steer": {
        await session.steer(String(msg.message));
        sendResponse(ws, sessionId, "steer", msg.id, true);
        break;
      }
      case "follow_up": {
        await session.followUp(String(msg.message));
        sendResponse(ws, sessionId, "follow_up", msg.id, true);
        break;
      }
      case "abort": {
        await session.abort();
        sendResponse(ws, sessionId, "abort", msg.id, true);
        break;
      }
      case "set_model": {
        await handleSetModel(ws, msg, state);
        break;
      }
      case "set_thinking_level": {
        session.setThinkingLevel(String(msg.level) as any);
        sendResponse(ws, sessionId, "set_thinking_level", msg.id, true);
        break;
      }
      case "new_session": {
        await state.runtime.newSession();
        state.unsubscribe();
        state.unsubscribe = subscribeToSession(ws, sessionId, state.runtime);
        await rebindExtensions(ws, sessionId, state);
        sendResponse(ws, sessionId, "new_session", msg.id, true);
        break;
      }
      case "navigate_tree": {
        log("navigate_tree:", msg.entryId);
        const result = await session.navigateTree(String(msg.entryId));
        log("navigate_tree result:", result);
        sendResponse(ws, sessionId, "navigate_tree", msg.id, true, result);
        break;
      }
      case "fork": {
        await state.runtime.fork(String(msg.entryId));
        state.unsubscribe();
        state.unsubscribe = subscribeToSession(ws, sessionId, state.runtime);
        await rebindExtensions(ws, sessionId, state);
        sendResponse(ws, sessionId, "fork", msg.id, true);
        break;
      }
      case "clone": {
        await state.runtime.fork(String(msg.entryId || ""), {
          position: "at",
        });
        state.unsubscribe();
        state.unsubscribe = subscribeToSession(ws, sessionId, state.runtime);
        await rebindExtensions(ws, sessionId, state);
        sendResponse(ws, sessionId, "clone", msg.id, true);
        break;
      }
      case "get_messages": {
        await handleGetMessages(ws, msg, state);
        break;
      }
      case "get_commands": {
        await handleGetCommands(ws, msg, state);
        break;
      }
      case "export_html": {
        try {
          const outputPath = msg.outputPath
            ? String(msg.outputPath)
            : undefined;
          const path = await session.exportToHtml(outputPath);
          sendResponse(ws, sessionId, "export_html", msg.id, true, { path });
        } catch (e: any) {
          sendResponse(
            ws,
            sessionId,
            "export_html",
            msg.id,
            false,
            undefined,
            e?.message || String(e)
          );
        }
        break;
      }
      case "compact": {
        await session.compact(
          msg.customInstructions ? String(msg.customInstructions) : undefined
        );
        sendResponse(ws, sessionId, "compact", msg.id, true);
        break;
      }
      case "extension_ui_response": {
        const pending = pendingExtensionRequests.get(msg.id);
        if (pending) {
          pendingExtensionRequests.delete(msg.id);
          pending.resolve(msg);
        } else {
          log("extension_ui_response with unknown id:", msg.id);
        }
        break;
      }
      default: {
        log("unknown command:", type);
        sendResponse(
          ws,
          sessionId,
          type,
          msg.id,
          false,
          undefined,
          "unknown command"
        );
      }
    }
  } catch (e: any) {
    log("command failed:", type, e);
    sendResponse(
      ws,
      sessionId,
      type,
      msg.id,
      false,
      undefined,
      e?.message || String(e)
    );
  }
}

function startServer(port: number) {
  wss = new WebSocketServer({ port }, () => {
    log("listening", port);
    console.log(`BRIDGE_PORT ${port}`);
  });

  wss.on("connection", (ws) => {
    if (clientSocket) {
      log("second client tried to connect; closing previous");
      clientSocket.close();
    }
    clientSocket = ws;
    log("client connected");

    // Process messages sequentially so create_session always finishes before
    // later commands like get_messages are handled.
    let processQueue: Promise<void> = Promise.resolve();
    ws.on("message", (data) => {
      processQueue = processQueue.then(async () => {
        try {
          const msg = JSON.parse(String(data));
          log("recv:", msg.type, msg.sessionId || "-");
          await dispatch(ws, msg);
        } catch (e) {
          log("failed to handle message:", e);
          send(ws, { type: "error", error: String(e) });
        }
      });
    });

    ws.on("close", () => {
      log("client disconnected");
      if (clientSocket === ws) {
        clientSocket = null;
      }
      for (const [sessionId, state] of sessions.entries()) {
        try {
          state.unsubscribe();
          state.runtime.dispose?.();
        } catch {
          // ignore
        }
      }
      sessions.clear();
      // Reject any pending extension UI dialog requests so extensions don't hang.
      for (const [, pending] of pendingExtensionRequests) {
        pending.reject(new Error("WebSocket closed"));
      }
      pendingExtensionRequests.clear();
      // Exit the bridge when the GUI disconnects so we do not leak processes.
      setTimeout(() => {
        log("exiting after disconnect");
        process.exit(0);
      }, 500);
    });

    ws.on("error", (err) => {
      log("websocket error:", err);
    });
  });
}

function parseArgs(args: string[]) {
  let port = 0;
  let agentDir = getAgentDir();
  for (let i = 0; i < args.length; i++) {
    switch (args[i]) {
      case "--port":
        port = parseInt(args[++i], 10) || 0;
        break;
      case "--agent-dir":
        agentDir = args[++i] || agentDir;
        break;
    }
  }
  return { port, agentDir };
}

async function main() {
  const { port, agentDir } = parseArgs(process.argv.slice(2));

  if (agentDir) {
    log("agentDir:", agentDir);
  }

  if (port) {
    startServer(port);
  } else {
    // Find a free port.
    const net = await import("node:net");
    const server = net.createServer();
    await new Promise<void>((resolve) => {
      server.listen(0, "127.0.0.1", () => {
        const addr = server.address();
        const freePort = typeof addr === "object" && addr ? addr.port : 0;
        server.close(() => {
          startServer(freePort);
          resolve();
        });
      });
    });
  }
}

main().catch((e) => {
  log("fatal:", e);
  process.exit(1);
});
