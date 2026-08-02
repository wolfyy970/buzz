import assert from "node:assert/strict";
import test from "node:test";

import { resolveAgentProjectAccessReadiness } from "./agentProjectAccessPolicy.ts";

const project = {
  id: "local-project",
  name: "Growth",
  projectChannelId: "growth-channel",
};
const requirement = {
  id: "analytics",
  label: "Analytics reports",
  capability: "mcp.tool.run_report",
  required: true,
};
const readyConnection = {
  id: "ga",
  capabilityIds: ["mcp.tool.run_report"],
  health: { status: "ready" },
};

function readiness(overrides = {}) {
  return resolveAgentProjectAccessReadiness({
    connections: [readyConnection],
    connectionsError: false,
    connectionsPending: false,
    draft: {
      projectId: project.id,
      connectionBindings: { analytics: readyConnection.id },
    },
    scopeAvailable: true,
    selectedProject: project,
    toolRequirements: [requirement],
    ...overrides,
  });
}

test("requires a Project with a discussion channel", () => {
  assert.equal(
    readiness({
      draft: { projectId: "", connectionBindings: {} },
    }).reason,
    "Choose a Project for this agent.",
  );
  assert.equal(
    readiness({
      selectedProject: { ...project, projectChannelId: null },
    }).reason,
    "Link a discussion channel to the selected Project.",
  );
});

test("blocks a required tool until a ready compatible connection is bound", () => {
  assert.equal(
    readiness({
      draft: { projectId: project.id, connectionBindings: {} },
    }).ready,
    false,
  );
  assert.equal(
    readiness({
      connections: [
        {
          ...readyConnection,
          capabilityIds: ["mcp.tool.export_report"],
        },
      ],
    }).ready,
    false,
  );
  assert.equal(readiness().ready, true);
});

test("optional tools do not block launch", () => {
  assert.equal(
    readiness({
      connections: [],
      draft: { projectId: project.id, connectionBindings: {} },
      toolRequirements: [{ ...requirement, required: false }],
    }).ready,
    true,
  );
});
