import { relayClient } from "@/shared/api/relayClient";
import { getRelayWsUrl, signRelayEvent } from "@/shared/api/tauri";
import { getIdentity } from "@/shared/api/tauriIdentity";
import { listProjectConnections } from "@/shared/api/tauriProjectConnections";
import { KIND_DELETION } from "@/shared/constants/kinds";

import { assertProjectHasNoConnections } from "./projectDeletionGuard";
import type { Project } from "./projectModels";

export async function deleteProject(project: Project): Promise<void> {
  const [identity, relayUrl] = await Promise.all([
    getIdentity(),
    getRelayWsUrl(),
  ]);
  if (identity.pubkey.toLowerCase() !== project.owner.toLowerCase()) {
    throw new Error("Only the project owner can delete this project.");
  }
  const projectScope = {
    relayUrl,
    operatorPubkey: identity.pubkey,
    projectAddress: project.projectAddress,
  };
  await assertProjectHasNoConnections(projectScope, listProjectConnections);
  const [currentIdentity, currentRelayUrl] = await Promise.all([
    getIdentity(),
    getRelayWsUrl(),
  ]);
  if (
    currentIdentity.pubkey.toLowerCase() !== identity.pubkey.toLowerCase() ||
    currentRelayUrl !== relayUrl
  ) {
    throw new Error(
      "The active community changed before the Project could be deleted.",
    );
  }

  const event = await signRelayEvent({
    kind: KIND_DELETION,
    content: `Delete project ${project.name}`,
    createdAt: nextProjectDeletionCreatedAt(project.createdAt),
    tags: [["a", project.projectAddress]],
  });
  if (event.pubkey.toLowerCase() !== project.owner.toLowerCase()) {
    throw new Error(
      "The active identity changed before the Project could be deleted.",
    );
  }

  await relayClient.publishEvent(
    event,
    "Timed out deleting project.",
    "Failed to delete project.",
    relayUrl,
  );
}

export function nextProjectDeletionCreatedAt(
  projectCreatedAt: number,
  nowSeconds = Math.floor(Date.now() / 1_000),
): number {
  return Math.max(nowSeconds, projectCreatedAt + 1);
}
