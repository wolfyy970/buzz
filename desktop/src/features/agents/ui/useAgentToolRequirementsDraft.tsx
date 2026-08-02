import * as React from "react";

import type {
  AgentToolRequirement,
  CreatePersonaInput,
  UpdatePersonaInput,
} from "@/shared/api/types";
import { AgentToolsSection } from "./AgentToolsSection";
import { agentToolRequirementIssues } from "./agentToolRequirements";
import type { AgentDefinitionDialogProps } from "./AgentDefinitionDialogTypes";

export function useAgentToolRequirementsDraft({
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
  const [requirements, setRequirements] = React.useState<
    AgentToolRequirement[]
  >(initialValues?.toolRequirements ?? []);

  React.useEffect(() => {
    if (open && initialValues) {
      setRequirements(initialValues.toolRequirements ?? []);
    }
  }, [initialValues, open]);

  const issues = agentToolRequirementIssues(requirements);
  const valid = issues.length === 0;
  const extraBlocked =
    typeof createSubmitBlocked === "function"
      ? createSubmitBlocked(requirements)
      : createSubmitBlocked;
  const extraReason =
    typeof createSubmitBlockReason === "function"
      ? createSubmitBlockReason(requirements)
      : createSubmitBlockReason;
  return {
    requirements,
    valid,
    createSubmitBlocked: Boolean(extraBlocked),
    submitBlockReason: !valid
      ? "Complete each tool name and capability ID."
      : isCreateMode
        ? (extraReason ?? null)
        : null,
    section: (
      <AgentToolsSection
        disabled={disabled}
        issues={issues}
        onChange={(nextRequirements) => {
          onUserChange();
          setRequirements(nextRequirements);
        }}
        value={requirements}
      />
    ),
  };
}
