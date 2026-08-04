import assert from "node:assert/strict";
import test from "node:test";

import {
  agentSkillsValid,
  agentSkillsValidationError,
  parseAgentSkills,
  synchronizeSkillMarkdown,
} from "./agentSkillTypes.ts";

const validSkill = {
  name: "campaign-analysis",
  description: "Analyze campaign performance and recommend the next action.",
  files: [
    {
      path: "SKILL.md",
      content: `---
name: campaign-analysis
description: Analyze campaign performance and recommend the next action.
---

# Campaign analysis

Compare performance with the prior period.`,
    },
    {
      path: "references/metrics.md",
      content: "Use conversion rate and qualified pipeline.",
    },
  ],
};

test("accepts a complete portable Skill", () => {
  assert.equal(agentSkillsValid([validSkill]), true);
});

test("rejects duplicate names and unsafe or duplicate file paths", () => {
  assert.equal(agentSkillsValid([validSkill, { ...validSkill }]), false);
  for (const path of [
    "../secret",
    "/absolute",
    "references\\secret",
    "references/../secret",
  ]) {
    assert.equal(
      agentSkillsValid([
        {
          ...validSkill,
          files: [validSkill.files[0], { path, content: "unsafe" }],
        },
      ]),
      false,
    );
  }
});

test("rejects cross-platform path collisions and reserved names", () => {
  for (const files of [
    [
      validSkill.files[0],
      { path: "references/README.md", content: "First" },
      { path: "references/readme.md", content: "Second" },
    ],
    [
      validSkill.files[0],
      { path: "references", content: "File" },
      { path: "references/readme.md", content: "Nested file" },
    ],
    [validSkill.files[0], { path: "references/CON.md", content: "Reserved" }],
  ]) {
    assert.equal(agentSkillsValid([{ ...validSkill, files }]), false);
  }
});

test("requires a lowercase load name, description, and SKILL.md", () => {
  assert.equal(
    agentSkillsValid([{ ...validSkill, name: "Campaign Skill" }]),
    false,
  );
  assert.equal(agentSkillsValid([{ ...validSkill, description: " " }]), false);
  assert.equal(
    agentSkillsValid([{ ...validSkill, description: " Padded summary" }]),
    false,
  );
  assert.equal(
    agentSkillsValid([{ ...validSkill, name: "double--hyphen" }]),
    false,
  );
  assert.equal(
    agentSkillsValid([
      {
        ...validSkill,
        files: [{ path: "references/metrics.md", content: "metrics" }],
      },
    ]),
    false,
  );
});

test("accepts standard Agent Skills metadata and rejects unknown fields", () => {
  const standard = {
    ...validSkill,
    files: [
      {
        path: "SKILL.md",
        content: `---
name: campaign-analysis
description: Analyze campaign performance and recommend the next action.
license: Apache-2.0
compatibility: Requires project analytics access
metadata:
  author: example-org
  version: "1.0"
allowed-tools: Read
---

# Campaign analysis`,
      },
    ],
  };
  assert.equal(agentSkillsValid([standard]), true);
  assert.equal(
    agentSkillsValid([
      {
        ...standard,
        files: [
          {
            ...standard.files[0],
            content: standard.files[0].content.replace(
              "\n---\n",
              "\nruntime-policy: unrestricted\n---\n",
            ),
          },
        ],
      },
    ]),
    false,
  );
});

test("rejects hidden instruction text and oversized files", () => {
  assert.equal(
    agentSkillsValid([
      {
        ...validSkill,
        files: [
          validSkill.files[0],
          {
            path: "references/hidden.md",
            content: "Review\u2066hidden direction",
          },
        ],
      },
    ]),
    false,
  );
  assert.equal(
    agentSkillsValid([
      {
        ...validSkill,
        files: [
          validSkill.files[0],
          { path: "references/large.md", content: "x".repeat(32 * 1_024 + 1) },
        ],
      },
    ]),
    false,
  );
});

test("accounts for JSON expansion in the complete bundle limit", () => {
  const quoted = '"'.repeat(32 * 1_024 - 128);
  assert.equal(
    agentSkillsValid([
      {
        ...validSkill,
        files: [
          validSkill.files[0],
          ...Array.from({ length: 4 }, (_, index) => ({
            path: `references/part-${index}.md`,
            content: quoted,
          })),
        ],
      },
    ]),
    false,
  );
});

test("requires matching SKILL.md frontmatter and rejects reserved names", () => {
  assert.equal(
    agentSkillsValid([
      {
        ...validSkill,
        files: [
          {
            path: "SKILL.md",
            content: "# Instructions without frontmatter",
          },
        ],
      },
    ]),
    false,
  );
  assert.equal(
    agentSkillsValid([
      {
        ...validSkill,
        description: "A different summary.",
      },
    ]),
    false,
  );
  assert.equal(
    agentSkillsValid([
      {
        ...validSkill,
        name: "buzz-cli",
      },
    ]),
    false,
  );
});

test("rejects likely credentials while allowing placeholders", () => {
  const secretSkill = {
    ...validSkill,
    files: [
      validSkill.files[0],
      {
        path: "references/private.md",
        content: "OPENAI_API_KEY=sk-live-real-looking-value",
      },
    ],
  };
  assert.equal(agentSkillsValid([secretSkill]), false);
  assert.match(agentSkillsValidationError([secretSkill]), /contain a secret/u);
  assert.equal(
    agentSkillsValid([
      {
        ...validSkill,
        files: [
          validSkill.files[0],
          {
            path: "references/setup.md",
            content: "OPENAI_API_KEY=sk-your-key-here",
          },
        ],
      },
    ]),
    true,
  );
});

test("keeps edited metadata aligned without discarding extra frontmatter", () => {
  const updated = synchronizeSkillMarkdown(
    `---
name: old-name
description: Old description
license: MIT
---

# Instructions`,
    "new-name",
    "New description",
  );
  assert.match(updated, /name: new-name/u);
  assert.match(updated, /description: New description/u);
  assert.match(updated, /license: MIT/u);
  assert.match(updated, /# Instructions/u);
});

test("parses valid wire values defensively and rejects partial arrays", () => {
  const parsed = parseAgentSkills([validSkill]);
  assert.deepEqual(parsed, [validSkill]);
  assert.notStrictEqual(parsed[0], validSkill);
  assert.notStrictEqual(parsed[0].files[0], validSkill.files[0]);
  assert.deepEqual(parseAgentSkills([validSkill, { name: "partial" }]), []);
  assert.deepEqual(parseAgentSkills([{ ...validSkill, executable: true }]), []);
  assert.deepEqual(
    parseAgentSkills([
      {
        ...validSkill,
        files: [{ ...validSkill.files[0], mode: "0755" }],
      },
    ]),
    [],
  );
});
