import { describe, expect, it } from "vitest";
import { normalizeCursorModelParams } from "../../src/core/overmind-model-params.js";

const grok47 = {
  id: "grok-4.7",
  parameters: [
    { id: "context", values: [{ value: "256k" }, { value: "500k" }] },
    { id: "reasoning_effort", values: ["low", "medium", "high", "xhigh"].map((value) => ({ value })) },
    { id: "fast", values: [{ value: "false" }, { value: "true" }] },
  ],
};
const kimi = { id: "kimi-k3", parameters: [{ id: "reasoning", values: ["low", "high", "max"].map((value) => ({ value })) }] };
const composer = { id: "composer-2.5", parameters: [{ id: "fast", values: [{ value: "false" }, { value: "true" }] }] };
const gpt55 = { id: "gpt-5.5", parameters: [{ id: "reasoning", values: ["none", "low", "medium", "high", "extra-high"].map((value) => ({ value })) }] };

describe("normalizeCursorModelParams", () => {
  it("maps generic effort onto reasoning_effort", () => {
    expect(normalizeCursorModelParams([{ id: "effort", value: "high" }], grok47)).toEqual([{ id: "reasoning_effort", value: "high" }]);
  });
  it("clamps ultra to the highest available level", () => {
    expect(normalizeCursorModelParams([{ id: "effort", value: "ultra" }], grok47)).toEqual([{ id: "reasoning_effort", value: "xhigh" }]);
    expect(normalizeCursorModelParams([{ id: "effort", value: "ultra" }], kimi)).toEqual([{ id: "reasoning", value: "max" }]);
  });
  it("picks the nearest level, ties toward cheaper", () => {
    expect(normalizeCursorModelParams([{ id: "effort", value: "medium" }], kimi)).toEqual([{ id: "reasoning", value: "low" }]);
    expect(normalizeCursorModelParams([{ id: "effort", value: "xhigh" }], gpt55)).toEqual([{ id: "reasoning", value: "extra-high" }]);
  });
  it("drops effort for models without an effort parameter and for none", () => {
    expect(normalizeCursorModelParams([{ id: "effort", value: "high" }], composer)).toEqual([]);
    expect(normalizeCursorModelParams([{ id: "effort", value: "none" }], grok47)).toEqual([]);
    expect(normalizeCursorModelParams([{ id: "effort", value: "none" }], gpt55)).toEqual([{ id: "reasoning", value: "none" }]);
  });
  it("keeps declared params and drops undeclared ones", () => {
    expect(
      normalizeCursorModelParams([{ id: "fast", value: "true" }, { id: "bogus", value: "1" }], grok47),
    ).toEqual([{ id: "fast", value: "true" }]);
  });
  it("passes through when the model is unknown", () => {
    expect(normalizeCursorModelParams([{ id: "effort", value: "high" }], undefined)).toEqual([{ id: "effort", value: "high" }]);
  });
});
