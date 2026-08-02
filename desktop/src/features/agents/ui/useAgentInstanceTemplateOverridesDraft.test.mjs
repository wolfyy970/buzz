import assert from "node:assert/strict";
import test from "node:test";

import { agentTemplateOverrideUpdate } from "./useAgentInstanceTemplateOverridesDraft.tsx";

const templateSkill = {
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
const template = {
  systemPrompt: "Use the template instructions.",
  skills: [templateSkill],
};
const agent = {
  systemPrompt: null,
  instructionsChangedForAgent: false,
  skills: [],
  skillsChangedForAgent: false,
};

test("sends private instruction and Skill overrides without resetting", () => {
  const privateSkill = {
    ...templateSkill,
    description: "Analyze campaigns privately.",
  };
  assert.deepEqual(
    agentTemplateOverrideUpdate({
      agent,
      instructions: "Use private instructions.",
      resetInstructions: false,
      resetSkills: false,
      skills: [privateSkill],
      template,
    }),
    {
      systemPrompt: "Use private instructions.",
      resetSystemPromptToTemplate: undefined,
      skills: [privateSkill],
      resetSkillsToTemplate: undefined,
    },
  );
});

test("uses explicit reset flags for existing private overrides", () => {
  assert.deepEqual(
    agentTemplateOverrideUpdate({
      agent: {
        ...agent,
        systemPrompt: "Private instructions.",
        instructionsChangedForAgent: true,
        skills: [templateSkill],
        skillsChangedForAgent: true,
      },
      instructions: template.systemPrompt,
      resetInstructions: true,
      resetSkills: true,
      skills: template.skills,
      template,
    }),
    {
      systemPrompt: undefined,
      resetSystemPromptToTemplate: true,
      skills: undefined,
      resetSkillsToTemplate: true,
    },
  );
});
