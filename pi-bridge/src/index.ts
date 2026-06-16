import { WebSocketServer, WebSocket } from "ws";
import * as fs from "node:fs";
import * as path from "node:path";
import {
  type CreateAgentSessionRuntimeFactory,
  createAgentSessionFromServices,
  createAgentSessionRuntime,
  createAgentSessionServices,
  getAgentDir,
  SessionManager,
  AuthStorage,
  ModelRegistry,
  DefaultResourceLoader,
} from "@earendil-works/pi-coding-agent";


const authStorage = AuthStorage.create();
const modelRegistry = ModelRegistry.create(authStorage);

interface SessionState {
  runtime: any;
  unsubscribe: () => void;
  cwd: string;
  agentDir: string;
}

const sessions = new Map<string, SessionState>();
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

function createRuntimeFactory(
  cwd: string
): CreateAgentSessionRuntimeFactory {
  return async ({ cwd: factoryCwd, sessionManager, sessionStartEvent }) => {
    const effectiveCwd = factoryCwd || cwd;
    const services = await createAgentSessionServices({ cwd: effectiveCwd });
    return {
      ...(await createAgentSessionFromServices({
        services,
        sessionManager,
        sessionStartEvent,
      })),
      services,
      diagnostics: services.diagnostics,
    };
  };
}

async function resolveModel(provider: string, modelId: string): Promise<any> {
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
    existing.unsubscribe();
    existing.runtime.dispose?.();
    sessions.delete(sessionId);
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
  const runtime = await createAgentSessionRuntime(createRuntimeFactory(cwd), {
    cwd,
    agentDir,
    sessionManager,
  });
  log("runtime created, sessionFile:", runtime.session.sessionFile);

  if (msg.model) {
    const [provider, modelId] = String(msg.model).split(":") as [
      string,
      string
    ];
    if (provider && modelId) {
      try {
        const model = await resolveModel(provider, modelId);
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
  sessions.set(sessionId, { runtime, unsubscribe, cwd, agentDir });

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

async function handleGetMessages(
  ws: WebSocket,
  msg: any,
  state: SessionState
): Promise<void> {
  const sessionFile = state.runtime.session.sessionFile;
  log("get_messages:", msg.sessionId, "sessionFile:", sessionFile);
  try {
    let messages: any[] = [];
    if (sessionFile && fs.existsSync(sessionFile)) {
      messages = await readSessionJsonl(sessionFile);
      log("read", messages.length, "messages from", sessionFile);
    } else {
      // Fallback to runtime agent state if available.
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

async function handleSetModel(
  ws: WebSocket,
  msg: any,
  state: SessionState
): Promise<void> {
  try {
    const provider = String(msg.provider);
    const modelId = String(msg.modelId);
    const model = await resolveModel(provider, modelId);
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
        sendResponse(ws, sessionId, "new_session", msg.id, true);
        break;
      }
      case "fork": {
        await state.runtime.fork(String(msg.entryId));
        state.unsubscribe();
        state.unsubscribe = subscribeToSession(ws, sessionId, state.runtime);
        sendResponse(ws, sessionId, "fork", msg.id, true);
        break;
      }
      case "clone": {
        await state.runtime.fork(String(msg.entryId || ""), {
          position: "at",
        });
        state.unsubscribe();
        state.unsubscribe = subscribeToSession(ws, sessionId, state.runtime);
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
        // SDK mode does not expose extension UI requests over this bridge.
        // Silently acknowledge to keep the client from retrying.
        sendResponse(ws, sessionId, "extension_ui_response", msg.id, true);
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
