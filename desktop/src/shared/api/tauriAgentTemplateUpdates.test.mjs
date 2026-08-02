import assert from "node:assert/strict";
import test from "node:test";

import { publishAgentTemplateVersionPayload } from "./tauriAgentTemplateUpdates.ts";

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
