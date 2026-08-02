import assert from "node:assert/strict";
import test from "node:test";

import {
  agentTemplateUpdateProgressLabel,
  applyAgentTemplateUpdatePayload,
  publishAgentTemplateVersionPayload,
} from "./tauriAgentTemplateUpdates.ts";

test("publish maps the saved template revision into the Tauri request", () => {
  assert.deepEqual(
    publishAgentTemplateVersionPayload({
      personaId: "analytics",
      expectedUpdatedAt: "2026-08-02T14:30:00.000Z",
    }),
    {
      input: {
        personaId: "analytics",
        expectedUpdatedAt: "2026-08-02T14:30:00.000Z",
      },
    },
  );
});

test("apply binds the frontend request id to the Tauri transaction", () => {
  const expectedVersion = {
    repoAddress: `30617:${"a".repeat(64)}:buzz-agent-templates`,
    commitOid: "b".repeat(40),
    artifactPath: "templates/analytics/versions/version/template.json",
    artifactSha256: "c".repeat(64),
  };
  assert.deepEqual(
    applyAgentTemplateUpdatePayload(
      {
        personaId: "analytics",
        expectedVersion,
        selectedPubkeys: ["d".repeat(64)],
        connectionBindingsByPubkey: {},
      },
      "4f62dd32-2f13-42ca-8c1d-c455149a0eef",
    ),
    {
      input: {
        requestId: "4f62dd32-2f13-42ca-8c1d-c455149a0eef",
        personaId: "analytics",
        expectedVersion,
        selectedPubkeys: ["d".repeat(64)],
        connectionBindingsByPubkey: {},
      },
    },
  );
});

test("progress stages use the settled product copy", () => {
  assert.deepEqual(
    [
      "preparing_update",
      "finishing_current_task",
      "starting_updated_agent",
      "checking_update",
      "updated",
      "update_rolled_back",
      "needs_attention",
    ].map(agentTemplateUpdateProgressLabel),
    [
      "Preparing update",
      "Finishing current task",
      "Starting updated agent",
      "Checking update",
      "Updated",
      "Update rolled back",
      "Needs attention",
    ],
  );
});
