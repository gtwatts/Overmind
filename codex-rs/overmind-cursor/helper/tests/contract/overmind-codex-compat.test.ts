import { afterEach, describe, expect, it } from "vitest";
import { parseResponsesRequest } from "../../src/protocols/openai-responses/parse.js";

const base = {
  model: "composer-2.5",
  input: [{ role: "user", content: [{ type: "input_text", text: "hi" }] }],
  stream: true,
  store: false,
};

afterEach(() => {
  delete process.env.OVERMIND_CODEX_COMPAT;
});

describe("Overmind Codex compatibility", () => {
  it("flattens top-level namespace tools into qualified client tools", () => {
    const parsed = parseResponsesRequest({
      ...base,
      tools: [
        {
          type: "namespace",
          name: "mcp__photocraft",
          description: "Photocraft",
          tools: [{ type: "function", name: "doc_new", description: "new doc", parameters: { type: "object", properties: {} } }],
        },
      ],
    }, { hostedSearchMode: "off" });
    expect(parsed.parsed.tools.map((tool) => [tool.name, tool.sdk_name, tool.namespace])).toEqual([
      ["doc_new", "mcp__photocraft__doc_new", "mcp__photocraft"],
    ]);
  });

  it("drops OpenAI-hosted tools and maps web_search onto Cursor search in compat mode", () => {
    process.env.OVERMIND_CODEX_COMPAT = "1";
    const tools = [
      { type: "web_search", external_web_access: true },
      { type: "image_generation", output_format: "png" },
      { type: "function", name: "exec_command", parameters: { type: "object", properties: { cmd: { type: "string" } } } },
    ];
    const off = parseResponsesRequest({ ...base, tools }, { hostedSearchMode: "off" });
    expect(off.parsed.tools.map((tool) => tool.name)).toEqual(["exec_command"]);
    expect(off.parsed.hostedSearch ?? false).toBe(false);
    const auto = parseResponsesRequest({ ...base, tools }, { hostedSearchMode: "auto" });
    expect(auto.parsed.tools.map((tool) => tool.name)).toEqual(["exec_command"]);
    expect(auto.parsed.hostedSearch).toBe(true);
  });

  it("keeps upstream fail-closed behavior without compat mode", () => {
    expect(() =>
      parseResponsesRequest({ ...base, tools: [{ type: "image_generation" }] }, { hostedSearchMode: "off" }),
    ).toThrow();
  });
});
