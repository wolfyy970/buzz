import assert from "node:assert/strict";
import test from "node:test";
import React from "react";
import { renderToStaticMarkup } from "react-dom/server";

import {
  AgentTemplateImpactPreview,
  agentTemplateUsageLabel,
} from "./AgentTemplateImpactPreview.tsx";

const agents = [
  { pubkey: "atlas-pubkey", name: "Atlas" },
  { pubkey: "beacon-pubkey", name: "Beacon" },
];

test("template impact copy gives an exact count", () => {
  assert.equal(agentTemplateUsageLabel(0), "No agents use this template yet");
  assert.equal(agentTemplateUsageLabel(1), "Used by 1 agent");
  assert.equal(agentTemplateUsageLabel(2), "Used by 2 agents");
});

test("template impact preview names every linked agent", () => {
  const markup = renderToStaticMarkup(
    React.createElement(AgentTemplateImpactPreview, { agents }),
  );

  assert.match(markup, /Used by 2 agents/);
  assert.match(markup, />Atlas</);
  assert.match(markup, />Beacon</);
  assert.match(markup, /template-impact-agent-atlas-pubkey/);
  assert.match(markup, /template-impact-agent-beacon-pubkey/);
});

test("empty template impact preview does not render an agent list", () => {
  const markup = renderToStaticMarkup(
    React.createElement(AgentTemplateImpactPreview, { agents: [] }),
  );

  assert.match(markup, /No agents use this template yet/);
  assert.doesNotMatch(markup, /role="list"/);
});
