import type { AgentToolRequirement } from "@/shared/api/types";

const UNSAFE_RECORD_KEYS = new Set(["__proto__", "constructor", "prototype"]);

export function agentToolRequirementsValid(
  requirements: readonly AgentToolRequirement[],
) {
  const ids = new Set<string>();
  return requirements.every((requirement) => {
    const valid =
      /^[A-Za-z0-9_-]{1,64}$/.test(requirement.id) &&
      !UNSAFE_RECORD_KEYS.has(requirement.id) &&
      requirement.label.trim().length > 0 &&
      /^mcp\.tool\.[A-Za-z0-9_.-]+$/.test(requirement.capability) &&
      !ids.has(requirement.id);
    ids.add(requirement.id);
    return valid;
  });
}
