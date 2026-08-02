import assert from "node:assert/strict";
import test from "node:test";

import { isInternalAgentTemplateProject } from "./internalAgentTemplateProject.ts";

const OWNER = "a".repeat(64);

function project(overrides = {}) {
  return {
    dtag: "buzz-agent-templates",
    owner: OWNER,
    purpose: "agent-templates",
    ...overrides,
  };
}

test("hides only the current owner's internal template repository", () => {
  assert.equal(isInternalAgentTemplateProject(project(), OWNER), true);
});

test("keeps a foreign repository with the reserved id visible", () => {
  assert.equal(
    isInternalAgentTemplateProject(project(), "b".repeat(64)),
    false,
  );
});

test("keeps an owner repository without the internal purpose visible", () => {
  assert.equal(
    isInternalAgentTemplateProject(project({ purpose: null }), OWNER),
    false,
  );
});

test("keeps an owner repository with a different id visible", () => {
  assert.equal(
    isInternalAgentTemplateProject(project({ dtag: "customer-agents" }), OWNER),
    false,
  );
});
