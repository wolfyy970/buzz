import * as React from "react";

import type { AgentSkill } from "@/shared/api/agentSkillTypes";
import type {
  CreatePersonaInput,
  UpdatePersonaInput,
} from "@/shared/api/types";
import { agentSkillsValidationError } from "@/shared/api/agentSkillTypes";
import { AgentSkillsSection } from "./AgentSkillsSection";
import type { AgentDefinitionDialogProps } from "./AgentDefinitionDialogTypes";
import { useAgentToolRequirementsDraft } from "./useAgentToolRequirementsDraft";

export function useAgentTemplateResourcesDraft({
  disabled,
  createSubmitBlocked,
  createSubmitBlockReason,
  initialValues,
  isCreateMode,
  onUserChange,
  open,
}: {
  disabled: boolean;
  createSubmitBlocked: AgentDefinitionDialogProps["createSubmitBlocked"];
  createSubmitBlockReason: AgentDefinitionDialogProps["createSubmitBlockReason"];
  initialValues: CreatePersonaInput | UpdatePersonaInput | null;
  isCreateMode: boolean;
  onUserChange: () => void;
  open: boolean;
}) {
  const tools = useAgentToolRequirementsDraft({
    disabled,
    createSubmitBlocked,
    createSubmitBlockReason,
    initialValues,
    isCreateMode,
    onUserChange,
    open,
  });
  const [skills, setSkills] = React.useState<AgentSkill[]>(
    initialValues?.skills ?? [],
  );

  React.useEffect(() => {
    if (open && initialValues) setSkills(initialValues.skills ?? []);
  }, [initialValues, open]);

  const skillsValidationError = agentSkillsValidationError(skills);
  const skillsValid = skillsValidationError === null;
  return {
    requirements: tools.requirements,
    skills,
    valid: tools.valid && skillsValid,
    createSubmitBlocked: tools.createSubmitBlocked,
    submitBlockReason: !tools.valid
      ? tools.submitBlockReason
      : !skillsValid
        ? skillsValidationError
        : tools.submitBlockReason,
    section: (
      <div className="space-y-5">
        <AgentSkillsSection
          disabled={disabled}
          onChange={(nextSkills) => {
            onUserChange();
            setSkills(nextSkills);
          }}
          value={skills}
        />
        {tools.section}
      </div>
    ),
  };
}
