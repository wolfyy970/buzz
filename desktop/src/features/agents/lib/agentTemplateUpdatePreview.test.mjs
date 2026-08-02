import assert from "node:assert/strict";
import test from "node:test";

import { hasOutdatedAgentTemplateInstances } from "./agentTemplateUpdatePreview.ts";

function preview(agents) {
  return {
    personaId: "persona-1",
    personaName: "Analytics",
    targetVersion: "version-2",
    agents,
  };
}

test("opens a review when every outdated agent is blocked", () => {
  assert.equal(
    hasOutdatedAgentTemplateInstances(
      preview([
        {
          currentVersion: "version-1",
          eligible: false,
          blockedReason: "This agent is running on another machine.",
        },
      ]),
    ),
    true,
  );
});

test("does not open a review when every linked agent is current", () => {
  assert.equal(
    hasOutdatedAgentTemplateInstances(
      preview([
        {
          currentVersion: "version-2",
          eligible: false,
          blockedReason: "This agent is running on another machine.",
        },
      ]),
    ),
    false,
  );
});
