import assert from "node:assert/strict";
import test from "node:test";
import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";

import {
  AgentTemplateInstructionChangesSummary,
  commonAgentInstructionChange,
} from "./AgentTemplateInstructionUpdateFields.tsx";

const change = {
  before: "Review the current report.",
  after:
    "||Show this literally.||\n[Review link](https://example.test)\n![Review image](https://example.test/image.png)",
  privateOverridePreserved: false,
};

test("groups instruction changes only when every byte and override state match", () => {
  assert.deepEqual(
    commonAgentInstructionChange([
      { instructionChange: change },
      { instructionChange: { ...change } },
    ]),
    change,
  );
  assert.equal(
    commonAgentInstructionChange([
      { instructionChange: change },
      {
        instructionChange: {
          ...change,
          privateOverridePreserved: true,
        },
      },
    ]),
    null,
  );
  assert.equal(
    commonAgentInstructionChange([
      { instructionChange: change },
      { instructionChange: null },
    ]),
    null,
  );
});

test("renders instruction changes literally instead of projecting Markdown", () => {
  const html = renderToStaticMarkup(
    createElement(AgentTemplateInstructionChangesSummary, { change }),
  );

  assert.match(html, /\|\|Show this literally\.\|\|/);
  assert.match(html, /\[Review link\]\(https:\/\/example\.test\)/);
  assert.match(
    html,
    /!\[Review image\]\(https:\/\/example\.test\/image\.png\)/,
  );
  assert.doesNotMatch(html, /<a(?:\s|>)/);
  assert.doesNotMatch(html, /<img(?:\s|>)/);
});

test("explains that a private override remains in control", () => {
  const html = renderToStaticMarkup(
    createElement(AgentTemplateInstructionChangesSummary, {
      change: { ...change, privateOverridePreserved: true },
    }),
  );

  assert.match(html, /This agent keeps its private instructions/);
  assert.match(
    html,
    /The new template instructions will be used if you reset it later/,
  );
});
