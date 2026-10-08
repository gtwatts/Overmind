// Overmind: client-executed `tool_search` support for the Responses surface.
//
// Codex defers MCP/plugin tool schemas for models with `supports_search_tool`
// and advertises a single `tool_search` tool instead. Codex executes the
// search itself: the model emits a `tool_search_call` output item, Codex
// answers with a `tool_search_output` input item listing the matching tool
// specs. The Cursor SDK has no native notion of this, so the helper exposes
// `tool_search` as an ordinary client custom tool, re-encodes calls to it as
// `tool_search_call` items, and folds the tools returned in
// `tool_search_output` into the executable catalog (which makes the run
// coordinator rebuild the Cursor agent with the newly loaded tools).
import { invalidRequest } from "../../errors.js";
import type { AnthropicContentBlock, AnthropicTool } from "../anthropic/types.js";

export const TOOL_SEARCH_TOOL_NAME = "tool_search";
const DESCRIPTION_PREVIEW_CHARS = 240;

type ToolUseBlock = Extract<AnthropicContentBlock, { type: "tool_use" }>;
type ToolResultBlock = Extract<AnthropicContentBlock, { type: "tool_result" }>;

export function parseToolSearchTool(raw: Record<string, unknown>): AnthropicTool {
  const parameters =
    raw.parameters && typeof raw.parameters === "object" && !Array.isArray(raw.parameters)
      ? (raw.parameters as Record<string, unknown>)
      : {
          type: "object",
          properties: {
            query: { type: "string", description: "Search query for deferred tools." },
            limit: { type: "number", description: "Maximum number of tools to return." },
          },
          required: ["query"],
          additionalProperties: false,
        };
  return {
    name: TOOL_SEARCH_TOOL_NAME,
    description: typeof raw.description === "string" ? raw.description : undefined,
    input_schema: parameters,
    tool_kind: "tool_search",
  };
}

export function parseToolSearchCall(raw: Record<string, unknown>): ToolUseBlock {
  const callId = typeof raw.call_id === "string" && raw.call_id.trim() ? raw.call_id : undefined;
  if (!callId) throw invalidRequest("tool_search_call must include call_id");
  return {
    type: "tool_use",
    id: callId,
    name: TOOL_SEARCH_TOOL_NAME,
    input: parseSearchArguments(raw.arguments),
    tool_kind: "tool_search",
  };
}

function parseSearchArguments(value: unknown): unknown {
  if (value === undefined || value === null || value === "") return {};
  if (typeof value === "string") {
    try {
      return JSON.parse(value) as unknown;
    } catch {
      throw invalidRequest("tool_search_call.arguments must be valid JSON");
    }
  }
  return value;
}

/**
 * Converts a `tool_search_output` input item into a tool_result block (a text
 * listing of what was loaded, for the model) plus the loaded tool specs (for
 * the executable catalog).
 */
export function parseToolSearchOutput(
  raw: Record<string, unknown>,
  parseTool: (value: unknown) => AnthropicTool[],
): { result: ToolResultBlock; tools: AnthropicTool[] } {
  const callId = typeof raw.call_id === "string" && raw.call_id.trim() ? raw.call_id : undefined;
  if (!callId) throw invalidRequest("tool_search_output must include call_id");
  const specs = Array.isArray(raw.tools) ? raw.tools : [];
  const tools = specs.flatMap(parseTool);
  return {
    result: {
      type: "tool_result",
      tool_use_id: callId,
      content: describeLoadedTools(tools),
      is_error: raw.status === "failed" || raw.status === "incomplete",
    },
    tools,
  };
}

export function describeLoadedTools(tools: AnthropicTool[]): string {
  if (tools.length === 0) {
    return "tool_search found no matching tools. Try a different query, or continue without them.";
  }
  const lines = tools.map((tool) => {
    const name = tool.sdk_name ?? tool.name;
    const description = (tool.description ?? "").replace(/\s+/g, " ").trim();
    const preview =
      description.length > DESCRIPTION_PREVIEW_CHARS
        ? `${description.slice(0, DESCRIPTION_PREVIEW_CHARS)}…`
        : description;
    return preview ? `- ${name}: ${preview}` : `- ${name}`;
  });
  return [
    `Loaded ${tools.length} tool(s). They are now available to call directly by these names:`,
    ...lines,
  ].join("\n");
}

export function encodeToolSearchCallItem(
  block: ToolUseBlock,
  status = "completed",
): Record<string, unknown> {
  const input = block.input && typeof block.input === "object" && !Array.isArray(block.input)
    ? block.input
    : {};
  return {
    type: "tool_search_call",
    call_id: block.id,
    status,
    execution: "client",
    arguments: input,
  };
}
