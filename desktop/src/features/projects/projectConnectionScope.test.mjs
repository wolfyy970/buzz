import assert from "node:assert/strict";
import test from "node:test";

import {
  agentProjectScopeAddress,
  projectConnectionAddress,
  projectConnectionScope,
  projectMatchesConnectionAddress,
} from "./projectConnectionScope.ts";

function project(overrides = {}) {
  return {
    id: "project-id",
    dtag: "project",
    name: "Project",
    description: "",
    cloneUrls: [],
    webUrl: null,
    owner: "owner",
    contributors: [],
    createdAt: 0,
    projectChannelId: "channel-id",
    status: "active",
    defaultBranch: "main",
    repoAddress: "30617:owner:repo",
    ...overrides,
  };
}

test("uses the explicit Project address when available", () => {
  const value = project({
    projectAddress: "30621:owner:project",
    repoAddress: "30617:owner:primary-repo",
  });

  assert.equal(projectConnectionAddress(value), "30621:owner:project");
  assert.equal(
    projectMatchesConnectionAddress(value, "30621:owner:project"),
    true,
  );
  assert.equal(
    projectMatchesConnectionAddress(value, "30617:owner:primary-repo"),
    false,
  );
});

test("falls back to the repository address for a legacy Project", () => {
  const value = project();

  assert.equal(projectConnectionAddress(value), "30617:owner:repo");
  assert.equal(
    projectMatchesConnectionAddress(value, "30617:owner:repo"),
    true,
  );
});

test("reads both current and legacy agent scope addresses", () => {
  assert.equal(
    agentProjectScopeAddress({
      ...projectConnectionScope({
        operatorPubkey: "operator",
        project: project(),
        relayUrl: "wss://relay.example",
      }),
      projectAddress: "30621:owner:project",
    }),
    "30621:owner:project",
  );
  assert.equal(
    agentProjectScopeAddress({
      relayUrl: "wss://relay.example",
      operatorPubkey: "operator",
      repoAddress: "30617:owner:repo",
      channelId: "channel-id",
    }),
    "30617:owner:repo",
  );
});

test("builds a scope only when the Project identity is complete", () => {
  assert.deepEqual(
    projectConnectionScope({
      operatorPubkey: "operator",
      project: project({ projectAddress: "30621:owner:project" }),
      relayUrl: "wss://relay.example",
    }),
    {
      relayUrl: "wss://relay.example",
      operatorPubkey: "operator",
      projectAddress: "30621:owner:project",
      channelId: "channel-id",
    },
  );
  assert.equal(
    projectConnectionScope({
      operatorPubkey: "operator",
      project: project({ projectChannelId: null }),
      relayUrl: "wss://relay.example",
    }),
    null,
  );
});
