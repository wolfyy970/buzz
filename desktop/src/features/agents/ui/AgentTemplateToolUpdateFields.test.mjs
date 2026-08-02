import assert from "node:assert/strict";
import test from "node:test";

import {
  bindingsForToolRequirements,
  commonAgentToolChanges,
  updatedToolRequirements,
} from "./AgentTemplateToolUpdateFields.tsx";

test("returns added and changed target requirements for rollout binding", () => {
  const added = {
    id: "analytics",
    label: "Analytics reports",
    capability: "mcp.tool.run_report",
    required: true,
  };
  const changed = {
    id: "issues",
    label: "Issue updates",
    capability: "mcp.tool.update_issue",
    required: true,
  };
  assert.deepEqual(
    updatedToolRequirements({
      added: [added],
      changed: [
        {
          before: { ...changed, capability: "mcp.tool.read_issue" },
          after: changed,
        },
      ],
      removed: [
        {
          id: "legacy",
          label: "Legacy reports",
          capability: "mcp.tool.legacy",
          required: true,
        },
      ],
    }),
    [added, changed],
  );
});

test("keeps only bindings used by the complete target tool set", () => {
  assert.deepEqual(
    bindingsForToolRequirements(
      {
        analytics: "connection-analytics",
        removed: "connection-legacy",
      },
      [
        {
          id: "analytics",
          label: "Analytics reports",
          capability: "mcp.tool.run_report",
          required: true,
        },
      ],
    ),
    { analytics: "connection-analytics" },
  );
});

test("groups changes only when every agent moves from the same version", () => {
  const changes = {
    added: [],
    changed: [],
    removed: [],
  };
  assert.deepEqual(
    commonAgentToolChanges([
      { toolChanges: changes },
      { toolChanges: { ...changes } },
    ]),
    changes,
  );
  assert.equal(
    commonAgentToolChanges([
      { toolChanges: changes },
      {
        toolChanges: {
          ...changes,
          removed: [
            {
              id: "legacy",
              label: "Legacy",
              capability: "mcp.tool.legacy",
              required: true,
            },
          ],
        },
      },
    ]),
    null,
  );
});
