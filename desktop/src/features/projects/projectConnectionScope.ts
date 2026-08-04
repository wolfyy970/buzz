import type { AgentProjectScope } from "@/shared/api/types";
import type { Project } from "./hooks";

type ProjectWithExplicitAddress = Project & {
  projectAddress?: string | null;
  repoAddress?: string | null;
};

type CompatibleAgentProjectScope = AgentProjectScope & {
  projectAddress?: string | null;
  repoAddress?: string | null;
};

/**
 * Returns the Project-level address used to scope shared connections.
 *
 * Legacy Projects are represented by one repository, so their repository
 * address remains the compatibility fallback. Multi-repository Projects carry
 * their own address and must not collapse connection ownership onto the
 * primary repository.
 */
export function projectConnectionAddress(project: Project): string {
  const compatibleProject = project as ProjectWithExplicitAddress;
  const explicitAddress = compatibleProject.projectAddress?.trim();
  return explicitAddress || compatibleProject.repoAddress?.trim() || "";
}

export function projectMatchesConnectionAddress(
  project: Project,
  address: string | null | undefined,
): boolean {
  return (
    Boolean(address?.trim()) &&
    projectConnectionAddress(project) === address?.trim()
  );
}

export function agentProjectScopeAddress(
  scope: AgentProjectScope | null | undefined,
): string {
  if (!scope) return "";
  const compatibleScope = scope as CompatibleAgentProjectScope;
  return (
    compatibleScope.projectAddress?.trim() ||
    compatibleScope.repoAddress?.trim() ||
    ""
  );
}

export function projectConnectionScope({
  operatorPubkey,
  project,
  relayUrl,
}: {
  operatorPubkey: string | null;
  project: Project | null;
  relayUrl: string | null;
}): AgentProjectScope | null {
  if (
    !project?.projectChannelId ||
    !relayUrl?.trim() ||
    !operatorPubkey?.trim()
  ) {
    return null;
  }

  const projectAddress = projectConnectionAddress(project);
  if (!projectAddress) return null;

  return {
    relayUrl: relayUrl.trim(),
    operatorPubkey: operatorPubkey.trim(),
    projectAddress,
    channelId: project.projectChannelId,
  };
}
