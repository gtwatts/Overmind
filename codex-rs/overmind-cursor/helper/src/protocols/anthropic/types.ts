/** Overmind: `tool_search` is Codex's client-executed deferred tool discovery. */
export type ToolKind = "function" | "custom" | "tool_search";

export type AnthropicRole = "user" | "assistant" | "system" | "developer" | "tool" | "function";

export type AnthropicContentBlock =
  | { type: "text"; text: string }
  | { type: "thinking"; thinking: string; signature?: string }
  | {
      type: "image";
      source: { type: "base64"; media_type: string; data: string } | { type: "url"; url: string };
    }
  | {
      type: "tool_use";
      id: string;
      name: string;
      input: unknown;
      tool_kind?: ToolKind;
      namespace?: string;
    }
  | {
      type: "tool_result";
      tool_use_id: string;
      content?: unknown;
      is_error?: boolean;
    };

export interface AnthropicMessage {
  role: AnthropicRole;
  content: string | AnthropicContentBlock[];
}

export interface AnthropicTool {
  name: string;
  description?: string;
  input_schema?: Record<string, unknown>;
  tool_kind?: ToolKind;
  sdk_name?: string;
  namespace?: string;
}

export interface AnthropicMessagesRequest {
  model: string;
  max_tokens?: number;
  system?: string | Array<{ type: "text"; text: string }>;
  messages: AnthropicMessage[];
  tools?: AnthropicTool[];
  stream?: boolean;
  metadata?: Record<string, unknown>;
  thinking?: unknown;
  reasoning_effort?: string;
  cursor_model_params?: Array<{ id: string; value: string }>;
}

export interface UsageView {
  input_tokens: number;
  output_tokens: number;
  cache_creation_input_tokens?: number;
  cache_read_input_tokens?: number;
  reasoning_tokens?: number;
  usage_deferred?: boolean;
  usage_status?: "sdk" | "unavailable" | "deferred";
  /** Overmind: Cursor run totals when input/output are last-step estimates. */
  run_input_tokens?: number;
  run_output_tokens?: number;
  run_cache_read_input_tokens?: number;
  run_cache_creation_input_tokens?: number;
  run_reasoning_output_tokens?: number;
  model_steps?: number;
}

export interface AssistantTurn {
  messageId: string;
  sessionId: string;
  model: string;
  stopReason: "end_turn" | "tool_use" | "max_tokens";
  blocks: AnthropicContentBlock[];
  usage: UsageView;
}

export interface ParsedToolResult {
  toolUseId: string;
  content: string;
  isError: boolean;
}

export interface ParsedMessages {
  model: string;
  modelParams: Array<{ id: string; value: string }>;
  stream: boolean;
  systemText: string;
  messages: AnthropicMessage[];
  tools: AnthropicTool[];
  images: Array<{ data: string; mimeType: string }>;
  lastUser: AnthropicMessage | undefined;
  continuation: ParsedToolResult[] | undefined;
  toolChoice: import("../tool-choice.js").ToolChoicePolicy;
  hostedSearch?: boolean;
  /** Overmind patch: Codex session/turn scope for replay keys (see overmind-client-scope.ts). */
  clientScope?: { session?: string; turn?: string };
}
