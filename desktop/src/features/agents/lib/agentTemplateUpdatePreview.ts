import type { AgentTemplateUpdatePreview } from "@/shared/api/tauriAgentTemplateUpdates";

export function hasOutdatedAgentTemplateInstances(
  preview: AgentTemplateUpdatePreview,
): boolean {
  return preview.agents.some(
    (agent) => agent.currentVersion !== preview.targetVersion,
  );
}
