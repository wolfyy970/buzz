import assert from "node:assert/strict";
import test from "node:test";

import React from "react";
import { renderToStaticMarkup } from "react-dom/server";

import { LifecycleActivity } from "./LifecycleActivity.tsx";

function renderPermission(options, { canCancel = false } = {}) {
  return renderToStaticMarkup(
    React.createElement(LifecycleActivity, {
      agentAvatarUrl: null,
      agentName: "Agent",
      agentPubkey: "agent-pubkey",
      item: {
        id: "permission:test",
        type: "lifecycle",
        title: "Permission requested",
        text: "Run the requested tool",
        timestamp: "2026-08-08T12:00:00.000Z",
        renderClass: "permission",
        actionable: true,
        requestNonce: "nonce",
        canCancelPermission: canCancel,
        channelId: "channel",
        options,
      },
    }),
  );
}

test("permission card renders only one-time decisions", () => {
  const html = renderPermission([
    { optionId: "allow", kind: "allow_once", label: "Allow once" },
    { optionId: "reject", kind: "reject_once", label: "Reject once" },
    {
      optionId: "persistent",
      kind: "allow_always",
      label: "Always allow",
    },
    { optionId: "future", kind: "future_scope", label: "Future choice" },
  ]);

  assert.match(html, /permission-decision-allow/);
  assert.match(html, /permission-decision-reject/);
  assert.doesNotMatch(html, /permission-decision-persistent/);
  assert.doesNotMatch(html, /permission-decision-future/);
  assert.doesNotMatch(html, /Always allow/);
  assert.doesNotMatch(html, /permission-decision-cancel/);
});

test("new harness allow-only permission card includes an immediate Cancel action", () => {
  const html = renderPermission(
    [{ optionId: "allow", kind: "allow_once", label: "Allow once" }],
    { canCancel: true },
  );

  assert.match(html, /permission-decision-allow/);
  assert.match(html, /permission-decision-cancel/);
  assert.match(html, />Cancel</);
});

test("legacy allow-only permission card does not show an unsupported Cancel action", () => {
  const html = renderPermission([
    { optionId: "allow", kind: "allow_once", label: "Allow once" },
  ]);

  assert.match(html, /permission-decision-allow/);
  assert.doesNotMatch(html, /permission-decision-cancel/);
});

test("permission card renders no action for persistent or unknown choices", () => {
  const html = renderPermission([
    { optionId: "persistent", kind: "allow_always" },
    { optionId: "future", kind: "future_scope" },
  ]);

  assert.doesNotMatch(html, /permission-decision-/);
  assert.doesNotMatch(html, />Allow</);
});
