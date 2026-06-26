import type { WebSocket } from "ws";
import type {
  DefaultResourceLoader,
  ModelRegistry,
} from "@earendil-works/pi-coding-agent";
import { DefaultResourceLoader as DefaultResourceLoaderCtor } from "@earendil-works/pi-coding-agent";
import { Type, type Static, type TSchema } from "typebox";
import { Value } from "typebox/value";
import type { Logger, SessionState, CommandWireInfo, ModelWireInfo } from "./types.js";
import { sendResponse, getMessagesFromSession } from "./messages.js";
import type { SessionStore } from "./session.js";

const BaseMessageSchema = Type.Object({
  type: Type.String(),
  sessionId: Type.Optional(Type.String()),
  id: Type.Optional(Type.String()),
});

const PromptSchema = Type.Intersect([
  BaseMessageSchema,
  Type.Object({
    type: Type.Literal("prompt"),
    message: Type.String(),
    images: Type.Optional(Type.Array(Type.Unknown())),
    streamingBehavior: Type.Optional(Type.String()),
  }),
]);

const SteerSchema = Type.Intersect([
  BaseMessageSchema,
  Type.Object({
    type: Type.Literal("steer"),
    message: Type.String(),
  }),
]);

const FollowUpSchema = Type.Intersect([
  BaseMessageSchema,
  Type.Object({
    type: Type.Literal("follow_up"),
    message: Type.String(),
  }),
]);

const SetModelSchema = Type.Intersect([
  BaseMessageSchema,
  Type.Object({
    type: Type.Literal("set_model"),
    provider: Type.String(),
    modelId: Type.String(),
  }),
]);

const SetThinkingLevelSchema = Type.Intersect([
  BaseMessageSchema,
  Type.Object({
    type: Type.Literal("set_thinking_level"),
    level: Type.String(),
  }),
]);

const NavigateTreeSchema = Type.Intersect([
  BaseMessageSchema,
  Type.Object({
    type: Type.Literal("navigate_tree"),
    entryId: Type.String(),
  }),
]);

const ForkSchema = Type.Intersect([
  BaseMessageSchema,
  Type.Object({
    type: Type.Literal("fork"),
    entryId: Type.String(),
  }),
]);

const CloneSchema = Type.Intersect([
  BaseMessageSchema,
  Type.Object({
    type: Type.Literal("clone"),
    entryId: Type.Optional(Type.String()),
  }),
]);

const ExportHtmlSchema = Type.Intersect([
  BaseMessageSchema,
  Type.Object({
    type: Type.Literal("export_html"),
    outputPath: Type.Optional(Type.String()),
  }),
]);

const CompactSchema = Type.Intersect([
  BaseMessageSchema,
  Type.Object({
    type: Type.Literal("compact"),
    customInstructions: Type.Optional(Type.String()),
  }),
]);

const GetModelSchema = Type.Intersect([
  BaseMessageSchema,
  Type.Object({
    type: Type.Literal("get_model"),
    provider: Type.Optional(Type.String()),
    modelId: Type.Optional(Type.String()),
  }),
]);

const GetSessionStatsSchema = Type.Intersect([
  BaseMessageSchema,
  Type.Object({
    type: Type.Literal("get_session_stats"),
  }),
]);

export type InboundMessage = Static<typeof BaseMessageSchema>;

export interface CommandContext {
  ws: WebSocket;
  sessionId: string;
  state: SessionState;
  msg: InboundMessage;
  store: SessionStore;
  modelRegistry: ModelRegistry;
  logger: Logger;
}

export interface GlobalCommandContext {
  ws: WebSocket;
  sessionId: string | undefined;
  msg: InboundMessage;
  modelRegistry: ModelRegistry;
  logger: Logger;
}

export type SessionCommandHandler = (ctx: CommandContext) => Promise<void>;
export type GlobalCommandHandler = (ctx: GlobalCommandContext) => Promise<void>;

function assertShape<T extends TSchema>(msg: InboundMessage, schema: T): Static<T> {
  if (!Value.Check(schema, msg)) {
    throw new Error("invalid message shape");
  }
  return msg as Static<T>;
}

export async function handlePrompt(ctx: CommandContext): Promise<void> {
  const { ws, sessionId, state, msg, logger } = ctx;
  const parsed = assertShape(msg, PromptSchema);
  const options: {
    images?: unknown[];
    streamingBehavior?: string;
    preflightResult?: (success: boolean) => void;
  } = {};
  if (parsed.images && parsed.images.length > 0) {
    options.images = parsed.images;
  }
  if (parsed.streamingBehavior) {
    options.streamingBehavior = parsed.streamingBehavior;
  }

  // Do NOT await the full turn — that would block the session message queue
  // and prevent abort/steer/follow_up from being handled while streaming.
  // The SDK streams events via the session subscription; we only need to
  // acknowledge once preflight succeeds (or fail early).
  let preflightSucceeded = false;
  options.preflightResult = (success: boolean) => {
    preflightSucceeded = success;
    if (success) {
      sendResponse(ws, sessionId, "prompt", parsed.id, true);
    }
  };

  state.runtime.session
    .prompt(parsed.message, options as never)
    .catch((e: unknown) => {
      const err = e instanceof Error ? e.message : String(e);
      logger.error("[bridge] prompt failed:", sessionId, err);
      if (!preflightSucceeded) {
        sendResponse(ws, sessionId, "prompt", parsed.id, false, undefined, err);
      }
    });
}

export async function handleSteer(ctx: CommandContext): Promise<void> {
  const { ws, sessionId, state, msg } = ctx;
  const parsed = assertShape(msg, SteerSchema);
  await state.runtime.session.steer(parsed.message);
  sendResponse(ws, sessionId, "steer", parsed.id, true);
}

export async function handleFollowUp(ctx: CommandContext): Promise<void> {
  const { ws, sessionId, state, msg } = ctx;
  const parsed = assertShape(msg, FollowUpSchema);
  await state.runtime.session.followUp(parsed.message);
  sendResponse(ws, sessionId, "follow_up", parsed.id, true);
}

export async function handleAbort(ctx: CommandContext): Promise<void> {
  const { ws, sessionId, state, msg, logger } = ctx;
  logger.info("[bridge] handleAbort start:", sessionId, "isStreaming:", state.runtime.session.isStreaming);
  try {
    await state.runtime.session.abort();
    logger.info("[bridge] handleAbort done:", sessionId, "isStreaming:", state.runtime.session.isStreaming);
  } catch (e: unknown) {
    const err = e instanceof Error ? e.message : String(e);
    logger.error("[bridge] handleAbort error:", sessionId, err);
  }
  sendResponse(ws, sessionId, "abort", msg.id, true);
}

export async function handleSetModel(ctx: CommandContext): Promise<void> {
  const { ws, sessionId, state, msg, modelRegistry } = ctx;
  const parsed = assertShape(msg, SetModelSchema);
  const model = modelRegistry.find(parsed.provider, parsed.modelId);
  if (!model) {
    throw new Error(`model not found: ${parsed.provider}:${parsed.modelId}`);
  }
  await state.runtime.session.setModel(model);
  sendResponse(ws, sessionId, "set_model", parsed.id, true);
}

export async function handleSetThinkingLevel(ctx: CommandContext): Promise<void> {
  const { ws, sessionId, state, msg } = ctx;
  const parsed = assertShape(msg, SetThinkingLevelSchema);
  state.runtime.session.setThinkingLevel(parsed.level as never);
  sendResponse(ws, sessionId, "set_thinking_level", parsed.id, true);
}

export async function handleNewSession(ctx: CommandContext): Promise<void> {
  const { ws, sessionId, state, msg, store } = ctx;
  await state.runtime.newSession();
  store.resubscribe(ws, sessionId, state);
  sendResponse(ws, sessionId, "new_session", msg.id, true);
}

export async function handleNavigateTree(ctx: CommandContext): Promise<void> {
  const { ws, sessionId, state, msg, logger } = ctx;
  const parsed = assertShape(msg, NavigateTreeSchema);
  logger.info("navigate_tree:", parsed.entryId);
  const result = await state.runtime.session.navigateTree(parsed.entryId);
  logger.info("navigate_tree result:", result);
  sendResponse(ws, sessionId, "navigate_tree", parsed.id, true, result);
}

export async function handleFork(ctx: CommandContext): Promise<void> {
  const { ws, sessionId, state, msg, store } = ctx;
  const parsed = assertShape(msg, ForkSchema);
  await state.runtime.fork(parsed.entryId);
  store.resubscribe(ws, sessionId, state);
  sendResponse(ws, sessionId, "fork", parsed.id, true);
}

export async function handleClone(ctx: CommandContext): Promise<void> {
  const { ws, sessionId, state, msg, store } = ctx;
  const parsed = assertShape(msg, CloneSchema);
  await state.runtime.fork(parsed.entryId || "", { position: "at" });
  store.resubscribe(ws, sessionId, state);
  sendResponse(ws, sessionId, "clone", parsed.id, true);
}

export async function handleGetMessages(ctx: CommandContext): Promise<void> {
  const { ws, sessionId, state, msg, logger } = ctx;
  const sessionFile = state.runtime.session.sessionFile;
  logger.info("get_messages:", sessionId, "sessionFile:", sessionFile);
  try {
    const fallback = (state.runtime.session.agent?.state?.messages as unknown[]) ?? [];
    const messages = await getMessagesFromSession(state.sessionManager, sessionFile, fallback);
    sendResponse(ws, sessionId, "get_messages", msg.id, true, { messages });
  } catch (e: unknown) {
    const err = e instanceof Error ? e.message : String(e);
    logger.error("get_messages failed:", err);
    sendResponse(ws, sessionId, "get_messages", msg.id, false, undefined, err);
  }
}

export async function handleGetCommands(ctx: CommandContext): Promise<void> {
  const { ws, sessionId, state, msg } = ctx;
  const loader = new DefaultResourceLoaderCtor({
    cwd: state.cwd,
    agentDir: state.agentDir,
  }) as DefaultResourceLoader;
  await loader.reload();
  const promptsResult = (loader.getPrompts?.() as { prompts?: unknown[] }) ?? { prompts: [] };
  const skillsResult = (loader.getSkills?.() as { skills?: unknown[] }) ?? { skills: [] };
  const prompts = promptsResult.prompts ?? [];
  const skills = skillsResult.skills ?? [];
  const commands: CommandWireInfo[] = [];
  for (const prompt of prompts) {
    const p = prompt as { name: string; description?: string; source?: string };
    commands.push({
      name: p.name,
      description: p.description,
      source: p.source || "prompt",
    });
  }
  for (const skill of skills) {
    const s = skill as { name: string; description?: string };
    commands.push({
      name: s.name,
      description: s.description,
      source: "skill",
    });
  }
  sendResponse(ws, sessionId, "get_commands", msg.id, true, { commands });
}

export async function handleExportHtml(ctx: CommandContext): Promise<void> {
  const { ws, sessionId, state, msg } = ctx;
  const parsed = assertShape(msg, ExportHtmlSchema);
  const outputPath = parsed.outputPath ? String(parsed.outputPath) : undefined;
  const path = await state.runtime.session.exportToHtml(outputPath);
  sendResponse(ws, sessionId, "export_html", parsed.id, true, { path });
}

export async function handleCompact(ctx: CommandContext): Promise<void> {
  const { ws, sessionId, state, msg } = ctx;
  const parsed = assertShape(msg, CompactSchema);
  await state.runtime.session.compact(
    parsed.customInstructions ? String(parsed.customInstructions) : undefined
  );
  sendResponse(ws, sessionId, "compact", parsed.id, true);
}

export async function handleExtensionUiResponse(ctx: CommandContext): Promise<void> {
  const { ws, sessionId, msg } = ctx;
  // SDK mode does not expose extension UI requests over this bridge.
  // Silently acknowledge to keep the client from retrying.
  sendResponse(ws, sessionId, "extension_ui_response", msg.id, true);
}

export async function handleGetModels(ctx: GlobalCommandContext): Promise<void> {
  const { ws, msg, modelRegistry } = ctx;
  const models = await modelRegistry.getAvailable();
  const wireModels: ModelWireInfo[] = models.map((m: unknown) => {
    const model = m as {
      provider: string;
      id: string;
      name: string;
      thinkingLevelMap?: Record<string, string>;
    };
    return {
      provider: model.provider,
      id: model.id,
      name: model.name,
      thinkingLevelMap: model.thinkingLevelMap,
    };
  });
  sendResponse(ws, ctx.sessionId || "bridge", "get_models", msg.id, true, {
    models: wireModels,
  });
}

export async function handleGetSessionStats(ctx: CommandContext): Promise<void> {
  const { ws, sessionId, state, msg } = ctx;
  const stats = state.runtime.session.getSessionStats();
  const contextUsage = state.runtime.session.getContextUsage();
  sendResponse(ws, sessionId, "get_session_stats", msg.id, true, {
    ...stats,
    contextUsage,
  });
}

export async function handleGetModel(ctx: GlobalCommandContext, state?: SessionState): Promise<void> {
  const { ws, msg, modelRegistry } = ctx;
  const parsed = assertShape(msg, GetModelSchema);
  let model: unknown;

  if (parsed.provider && parsed.modelId) {
    model = modelRegistry.find(parsed.provider, parsed.modelId);
    if (!model) {
      throw new Error(`model not found: ${parsed.provider}:${parsed.modelId}`);
    }
  } else if (state?.runtime.session.model) {
    model = state.runtime.session.model;
  } else {
    throw new Error("missing provider/modelId or active session model");
  }

  const m = model as { provider: string; id: string; name: string } | undefined;
  sendResponse(ws, ctx.sessionId || "bridge", "get_model", msg.id, true, {
    model: m
      ? {
          provider: m.provider,
          id: m.id,
          name: m.name,
        }
      : null,
  });
}

export const sessionCommands = new Map<string, SessionCommandHandler>([
  ["prompt", handlePrompt],
  ["steer", handleSteer],
  ["follow_up", handleFollowUp],
  ["abort", handleAbort],
  ["set_model", handleSetModel],
  ["set_thinking_level", handleSetThinkingLevel],
  ["new_session", handleNewSession],
  ["navigate_tree", handleNavigateTree],
  ["fork", handleFork],
  ["clone", handleClone],
  ["get_messages", handleGetMessages],
  ["get_commands", handleGetCommands],
  ["export_html", handleExportHtml],
  ["compact", handleCompact],
  ["extension_ui_response", handleExtensionUiResponse],
  ["get_session_stats", handleGetSessionStats],
]);
