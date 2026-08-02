import * as React from "react";

import { agentSkillsValidationIssue } from "@/shared/api/agentSkillTypes";
import type { AgentSkill } from "@/shared/api/agentSkillTypes";
import type {
  AgentPersona,
  ManagedAgent,
  UpdateManagedAgentInput,
} from "@/shared/api/types";
import { Badge } from "@/shared/ui/badge";
import { Button } from "@/shared/ui/button";
import { Textarea } from "@/shared/ui/textarea";
import { AgentSkillsSection } from "./AgentSkillsSection";

function skillsEqual(
  left: readonly AgentSkill[],
  right: readonly AgentSkill[],
) {
  return JSON.stringify(left) === JSON.stringify(right);
}

function initialInstructions(
  agent: ManagedAgent,
  template: AgentPersona | null,
) {
  return agent.instructionsChangedForAgent
    ? (agent.systemPrompt ?? "")
    : (template?.systemPrompt ?? agent.systemPrompt ?? "");
}

function initialSkills(agent: ManagedAgent, template: AgentPersona | null) {
  return agent.skillsChangedForAgent
    ? agent.skills
    : (template?.skills ?? agent.skills);
}

export function agentTemplateOverrideUpdate({
  agent,
  instructions,
  resetInstructions,
  resetSkills,
  skills,
  template,
}: {
  agent: ManagedAgent;
  instructions: string;
  resetInstructions: boolean;
  resetSkills: boolean;
  skills: AgentSkill[];
  template: AgentPersona;
}): Pick<
  UpdateManagedAgentInput,
  | "systemPrompt"
  | "resetSystemPromptToTemplate"
  | "skills"
  | "resetSkillsToTemplate"
> {
  const originalInstructions = initialInstructions(agent, template);
  const originalSkills = initialSkills(agent, template);
  return {
    systemPrompt:
      !resetInstructions && instructions !== originalInstructions
        ? instructions
        : undefined,
    resetSystemPromptToTemplate:
      resetInstructions && agent.instructionsChangedForAgent ? true : undefined,
    skills:
      !resetSkills && !skillsEqual(skills, originalSkills) ? skills : undefined,
    resetSkillsToTemplate:
      resetSkills && agent.skillsChangedForAgent ? true : undefined,
  };
}

export function useAgentInstanceTemplateOverridesDraft({
  agent,
  disabled,
  open,
  template,
}: {
  agent: ManagedAgent;
  disabled: boolean;
  open: boolean;
  template: AgentPersona | null;
}) {
  const [instructions, setInstructions] = React.useState(() =>
    initialInstructions(agent, template),
  );
  const [skills, setSkills] = React.useState<AgentSkill[]>(() =>
    initialSkills(agent, template),
  );
  const [resetInstructions, setResetInstructions] = React.useState(false);
  const [resetSkills, setResetSkills] = React.useState(false);
  const instructionsTouched = React.useRef(false);
  const skillsTouched = React.useRef(false);

  // biome-ignore lint/correctness/useExhaustiveDependencies: reset only when opening or switching agents; polling must not wipe edits
  React.useEffect(() => {
    if (!open) return;
    instructionsTouched.current = false;
    skillsTouched.current = false;
    setInstructions(initialInstructions(agent, template));
    setSkills(initialSkills(agent, template));
    setResetInstructions(false);
    setResetSkills(false);
  }, [open, agent.pubkey]);

  React.useEffect(() => {
    if (!open || !template) return;
    if (!instructionsTouched.current && !agent.instructionsChangedForAgent) {
      setInstructions(template.systemPrompt);
    }
    if (!skillsTouched.current && !agent.skillsChangedForAgent) {
      setSkills(template.skills);
    }
  }, [
    agent.instructionsChangedForAgent,
    agent.skillsChangedForAgent,
    open,
    template,
  ]);

  if (!template) {
    return {
      section: null,
      update: {},
      valid: true,
      submitBlockReason: null,
    };
  }

  const instructionsOverride =
    !resetInstructions &&
    (agent.instructionsChangedForAgent ||
      instructions !== template.systemPrompt);
  const skillsOverride =
    !resetSkills &&
    (agent.skillsChangedForAgent || !skillsEqual(skills, template.skills));
  const skillsValidationIssue = agentSkillsValidationIssue(skills);
  const valid = skillsValidationIssue === null;

  return {
    valid,
    submitBlockReason: skillsValidationIssue?.message ?? null,
    update: agentTemplateOverrideUpdate({
      agent,
      instructions,
      resetInstructions,
      resetSkills,
      skills,
      template,
    }),
    section: (
      <section
        className="space-y-4 rounded-xl border border-border/70 bg-muted/10 p-4"
        data-testid="agent-template-overrides"
      >
        <div>
          <h3 className="text-sm font-medium text-foreground">
            Instructions and Skills
          </h3>
          <p className="mt-1 text-xs text-muted-foreground">
            Changes here apply only to {agent.name}. Edit its template to update
            every linked agent.
          </p>
        </div>

        <div className="space-y-2">
          <div className="flex items-center justify-between gap-3">
            <div className="flex items-center gap-2">
              <label
                className="text-xs font-medium text-foreground"
                htmlFor="edit-agent-private-instructions"
              >
                Agent instructions
              </label>
              {instructionsOverride ? (
                <Badge variant="secondary">Changed for this agent</Badge>
              ) : null}
            </div>
            {instructionsOverride ? (
              <Button
                data-testid="reset-agent-instructions-to-template"
                disabled={disabled}
                onClick={() => {
                  instructionsTouched.current = true;
                  setInstructions(template.systemPrompt);
                  setResetInstructions(true);
                }}
                size="xs"
                type="button"
                variant="ghost"
              >
                Reset to template
              </Button>
            ) : null}
          </div>
          <Textarea
            className="min-h-32 resize-y"
            data-testid="edit-agent-private-instructions"
            disabled={disabled}
            id="edit-agent-private-instructions"
            onChange={(event) => {
              instructionsTouched.current = true;
              setResetInstructions(false);
              setInstructions(event.target.value);
            }}
            value={instructions}
          />
        </div>

        <AgentSkillsSection
          beforeAddAction={
            skillsOverride ? (
              <Button
                data-testid="reset-agent-skills-to-template"
                disabled={disabled}
                onClick={() => {
                  skillsTouched.current = true;
                  setSkills(template.skills);
                  setResetSkills(true);
                }}
                size="xs"
                type="button"
                variant="ghost"
              >
                Reset to template
              </Button>
            ) : null
          }
          description={`Add Skills only ${agent.name} can use.`}
          disabled={disabled}
          emptyMessage="This agent does not have any Skills."
          headingLevel="h4"
          onChange={(nextSkills) => {
            skillsTouched.current = true;
            setResetSkills(false);
            setSkills(nextSkills);
          }}
          status={
            skillsOverride ? (
              <Badge variant="secondary">Changed for this agent</Badge>
            ) : null
          }
          validationIssue={skillsValidationIssue}
          value={skills}
        />
      </section>
    ),
  };
}
