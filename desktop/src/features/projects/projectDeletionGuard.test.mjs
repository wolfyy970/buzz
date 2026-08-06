import assert from "node:assert/strict";
import test from "node:test";

import {
  assertProjectHasNoConnections,
  ProjectHasConnectionsError,
} from "./projectDeletionGuard.ts";

const projectScope = {
  relayUrl: "wss://relay.example",
  operatorPubkey: "a".repeat(64),
  projectAddress: `30621:${"a".repeat(64)}:analytics`,
};

test("Project deletion continues when the exact local scope has no Connections", async () => {
  let capturedScope;

  await assertProjectHasNoConnections(projectScope, async (scope) => {
    capturedScope = scope;
    return [];
  });

  assert.deepEqual(capturedScope, projectScope);
});

test("Project deletion is blocked while one local Connection remains", async () => {
  await assert.rejects(
    assertProjectHasNoConnections(projectScope, async () => [{ id: "ga4" }]),
    (error) =>
      error instanceof ProjectHasConnectionsError &&
      error.connectionCount === 1 &&
      /Remove this Project's connection/.test(error.message),
  );
});

test("Project deletion reports the full Connection impact", async () => {
  await assert.rejects(
    assertProjectHasNoConnections(projectScope, async () => [
      { id: "ga4" },
      { id: "search-console" },
      { id: "ads" },
    ]),
    (error) =>
      error instanceof ProjectHasConnectionsError &&
      error.connectionCount === 3 &&
      /3 connections/.test(error.message),
  );
});

test("Project deletion fails closed when local Connections cannot be checked", async () => {
  await assert.rejects(
    assertProjectHasNoConnections(projectScope, async () => {
      throw new Error("credential store unavailable");
    }),
    /credential store unavailable/,
  );
});
