import type { AgentTemplateUpdatePreview } from "@/shared/api/tauriAgentTemplateUpdates";

export function hasOutdatedAgentTemplateInstances(
  preview: AgentTemplateUpdatePreview,
): boolean {
  return preview.agents.some(
    (agent) => agent.currentVersion !== preview.targetVersionToken,
  );
}

export function shortAgentTemplateVersionToken(version: string | null): string {
  if (!version) return "unversioned";
  const commit =
    /^git:30617:[0-9a-f]{64}:buzz-agent-templates:([0-9a-f]{40}|[0-9a-f]{64}):/i.exec(
      version,
    )?.[1] ??
    /^git:([0-9a-f]{40}|[0-9a-f]{64}):/i.exec(version)?.[1] ??
    version;
  return commit.slice(0, 7);
}
