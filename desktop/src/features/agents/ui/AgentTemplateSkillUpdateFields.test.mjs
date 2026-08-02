import assert from "node:assert/strict";
import test from "node:test";

import { commonAgentSkillChanges } from "./AgentTemplateSkillUpdateFields.tsx";

const skill = {
  name: "campaign-analysis",
  description: "Analyze campaigns.",
  files: [
    {
      path: "SKILL.md",
      content:
        "---\nname: campaign-analysis\ndescription: Analyze campaigns.\n---\n\n# Campaign analysis",
    },
  ],
};

test("groups Skill changes only when every agent has the same diff", () => {
  const changes = {
    added: [skill],
    changed: [],
    removed: [],
  };
  assert.deepEqual(
    commonAgentSkillChanges([
      { skillChanges: changes },
      { skillChanges: { ...changes } },
    ]),
    changes,
  );
  assert.equal(
    commonAgentSkillChanges([
      { skillChanges: changes },
      { skillChanges: { added: [], changed: [], removed: [] } },
    ]),
    null,
  );
});
