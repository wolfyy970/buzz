import assert from "node:assert/strict";
import test from "node:test";

import { buildProjectConnectionSecretChanges } from "./projectConnectionSecrets.ts";

test("preserves new secret values byte-for-byte, including an empty value", () => {
  assert.deepEqual(
    buildProjectConnectionSecretChanges(
      [
        { key: " API_TOKEN ", value: "  value with spaces  " },
        { key: "EMPTY_VALUE", value: "" },
      ],
      [],
      [],
    ),
    {
      ok: true,
      env: {
        API_TOKEN: "  value with spaces  ",
        EMPTY_VALUE: "",
      },
      removeEnvKeys: [],
    },
  );
});

test("a blank existing secret retains its saved value", () => {
  assert.deepEqual(
    buildProjectConnectionSecretChanges(
      [{ key: "API_TOKEN", value: "" }],
      ["API_TOKEN"],
      [],
    ),
    { ok: true, env: {}, removeEnvKeys: [] },
  );
});

test("explicit removals stay separate from replacement values", () => {
  assert.deepEqual(
    buildProjectConnectionSecretChanges(
      [{ key: "API_TOKEN", value: "replacement" }],
      ["API_TOKEN"],
      ["API_TOKEN", "OLD_TOKEN"],
    ),
    {
      ok: true,
      env: { API_TOKEN: "replacement" },
      removeEnvKeys: ["OLD_TOKEN"],
    },
  );
});
