import assert from "node:assert/strict";
import test from "node:test";

import { fromRawPersona } from "./tauriPersonas.ts";

function rawPersona(overrides = {}) {
  return {
    id: "persona-1",
    display_name: "Team Analyst",
    avatar_url: null,
    system_prompt: "You are Team Analyst.",
    runtime: null,
    model: null,
    provider: null,
    name_pool: [],
    is_builtin: false,
    is_active: true,
    source_team: null,
    env_vars: {},
    created_at: "2026-01-01T00:00:00.000Z",
    updated_at: "2026-01-01T00:00:00.000Z",
    ...overrides,
  };
}

test("fromRawPersona maps source_team to sourceTeam", () => {
  const persona = fromRawPersona(rawPersona({ source_team: "team-research" }));

  assert.equal(persona.sourceTeam, "team-research");
});

test("fromRawPersona maps the latest published template version", () => {
  const publishedVersion = {
    repoAddress: `30617:${"a".repeat(64)}:buzz-agent-templates`,
    commitOid: "b".repeat(40),
    artifactPath: `templates/analytics/versions/${"c".repeat(64)}/template.json`,
    artifactSha256: "c".repeat(64),
  };

  const persona = fromRawPersona(
    rawPersona({ published_version: publishedVersion }),
  );

  assert.deepEqual(persona.publishedVersion, publishedVersion);
});

test("fromRawPersona defaults an unpublished template to null", () => {
  assert.equal(fromRawPersona(rawPersona()).publishedVersion, null);
});
