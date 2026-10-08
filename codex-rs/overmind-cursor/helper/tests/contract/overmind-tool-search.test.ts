import { describe, expect, it } from "vitest";
import { encodeResponsesOutput } from "../../src/protocols/openai-responses/encode.js";
import { parseResponsesRequest } from "../../src/protocols/openai-responses/parse.js";
import { describeLoadedTools } from "../../src/protocols/openai-responses/tool-search.js";

const toolSearchTool = {
  type: "tool_search",
  execution: "client",
  description: "# Tool discovery",
  parameters: {
    type: "object",
    properties: { query: { type: "string" }, limit: { type: "number" } },
    required: ["query"],
    additionalProperties: false,
  },
};

const user = { role: "user", content: [{ type: "input_text", text: "make an image" }] };

describe("Overmind tool_search (deferred plugin tools)", () => {
  it("exposes tool_search as a client tool instead of dropping it", () => {
    process.env.OVERMIND_CODEX_COMPAT = "1";
    try {
      const parsed = parseResponsesRequest(
        { model: "composer-2.5", input: [user], stream: true, store: false, tools: [toolSearchTool] },
        { hostedSearchMode: "off" },
      );
      expect(parsed.parsed.tools).toEqual([
        {
          name: "tool_search",
          description: "# Tool discovery",
          input_schema: toolSearchTool.parameters,
          tool_kind: "tool_search",
        },
      ]);
    } finally {
      delete process.env.OVERMIND_CODEX_COMPAT;
    }
  });

  it("folds tool_search_output tools into the catalog and history", () => {
    const parsed = parseResponsesRequest(
      {
        model: "composer-2.5",
        stream: true,
        store: false,
        tools: [toolSearchTool],
        input: [
          user,
          {
            type: "tool_search_call",
            call_id: "ts-1",
            execution: "client",
            status: "completed",
            arguments: { query: "photocraft new document", limit: 4 },
          },
          {
            type: "tool_search_output",
            call_id: "ts-1",
            status: "completed",
            execution: "client",
            tools: [
              {
                type: "namespace",
                name: "mcp__photocraft",
                description: "Photocraft image editor",
                tools: [
                  {
                    type: "function",
                    name: "doc_new",
                    description: "Create a new document.",
                    defer_loading: true,
                    parameters: { type: "object", properties: { width: { type: "number" } } },
                  },
                ],
              },
            ],
          },
        ],
      },
      { hostedSearchMode: "off" },
    );
    expect(parsed.parsed.tools.map((tool) => tool.sdk_name ?? tool.name)).toEqual([
      "tool_search",
      "mcp__photocraft__doc_new",
    ]);
    const messages = parsed.parsed.messages;
    const assistant = messages[messages.length - 2]!;
    const results = messages[messages.length - 1]!;
    expect(assistant.content).toEqual([
      {
        type: "tool_use",
        id: "ts-1",
        name: "tool_search",
        input: { query: "photocraft new document", limit: 4 },
        tool_kind: "tool_search",
      },
    ]);
    expect(results.role).toBe("user");
    const block = (results.content as Array<Record<string, unknown>>)[0]!;
    expect(block.type).toBe("tool_result");
    expect(block.tool_use_id).toBe("ts-1");
    expect(String(block.content)).toContain("mcp__photocraft__doc_new: Create a new document.");
  });

  it("encodes Cursor calls to tool_search as client tool_search_call items", () => {
    const output = encodeResponsesOutput({
      messageId: "msg_1",
      model: "composer-2.5",
      sessionId: "s",
      stopReason: "tool_use",
      usage: { input_tokens: 1, output_tokens: 1 },
      blocks: [
        {
          type: "tool_use",
          id: "call_9",
          name: "tool_search",
          input: { query: "ffmpeg probe" },
          tool_kind: "tool_search",
        },
      ],
    } as never);
    expect(output).toEqual([
      {
        type: "tool_search_call",
        call_id: "call_9",
        status: "completed",
        execution: "client",
        arguments: { query: "ffmpeg probe" },
      },
    ]);
  });

  it("explains empty searches", () => {
    expect(describeLoadedTools([])).toContain("no matching tools");
  });
});
