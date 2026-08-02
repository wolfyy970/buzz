import assert from "node:assert/strict";
import test from "node:test";

import {
  hasOutdatedAgentTemplateInstances,
  shortAgentTemplateVersionToken,
} from "./agentTemplateUpdatePreview.ts";

function preview(agents) {
  return {
    personaId: "persona-1",
    personaName: "Analytics",
    targetVersion: {
      repoAddress: `30617:${"a".repeat(64)}:buzz-agent-templates`,
      commitOid: "b".repeat(40),
      artifactPath: `templates/analytics/versions/${"c".repeat(64)}/template.json`,
      artifactSha256: "c".repeat(64),
    },
    targetVersionToken: "version-2",
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

test("shows the commit from a full immutable version token", () => {
  const token = `git:30617:${"a".repeat(64)}:buzz-agent-templates:${"b".repeat(
    40,
  )}:templates/analytics/versions/${"c".repeat(64)}/template.json:${"c".repeat(
    64,
  )}`;
  assert.equal(shortAgentTemplateVersionToken(token), "bbbbbbb");
  assert.equal(shortAgentTemplateVersionToken(null), "unversioned");
});
