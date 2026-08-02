export const AGENT_TEMPLATE_REPOSITORY_DTAG = "buzz-agent-templates";
export const AGENT_TEMPLATE_REPOSITORY_PURPOSE = "agent-templates";

type ProjectIdentity = {
  dtag: string;
  owner: string;
  purpose?: string | null;
};

export function isInternalAgentTemplateProject(
  project: ProjectIdentity,
  currentIdentityPubkey: string | null | undefined,
): boolean {
  const currentOwner = currentIdentityPubkey?.trim().toLowerCase();
  return (
    Boolean(currentOwner) &&
    project.owner.trim().toLowerCase() === currentOwner &&
    project.dtag === AGENT_TEMPLATE_REPOSITORY_DTAG &&
    project.purpose === AGENT_TEMPLATE_REPOSITORY_PURPOSE
  );
}
