import type { AgentToolRequirement } from "@/shared/api/types";

const UNSAFE_RECORD_KEYS = new Set(["__proto__", "constructor", "prototype"]);

export type AgentToolRequirementIssue = {
  field: "row" | "label" | "capability";
  index: number;
  message: string;
};

export function agentToolRequirementIssues(
  requirements: readonly AgentToolRequirement[],
): AgentToolRequirementIssue[] {
  const ids = new Set<string>();
  const issues: AgentToolRequirementIssue[] = [];
  for (const [index, requirement] of requirements.entries()) {
    if (
      !/^[A-Za-z0-9_-]{1,64}$/.test(requirement.id) ||
      UNSAFE_RECORD_KEYS.has(requirement.id) ||
      ids.has(requirement.id)
    ) {
      issues.push({
        field: "row",
        index,
        message: "Remove this tool and add it again.",
      });
    }
    ids.add(requirement.id);
    if (requirement.label.trim().length === 0) {
      issues.push({
        field: "label",
        index,
        message: "Enter a name people will recognize.",
      });
    }
    if (!/^mcp\.tool\.[A-Za-z0-9_.-]+$/.test(requirement.capability)) {
      issues.push({
        field: "capability",
        index,
        message: "Enter a capability ID beginning with mcp.tool.",
      });
    }
  }
  return issues;
}

export function agentToolRequirementsValid(
  requirements: readonly AgentToolRequirement[],
) {
  return agentToolRequirementIssues(requirements).length === 0;
}
