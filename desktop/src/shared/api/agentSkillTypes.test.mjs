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

test("requires a lowercase load name, description, and SKILL.md", () => {
  assert.equal(
    agentSkillsValid([{ ...validSkill, name: "Campaign Skill" }]),
    false,
  );
  assert.equal(agentSkillsValid([{ ...validSkill, description: " " }]), false);
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
});
