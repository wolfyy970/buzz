import { invokeTauri } from "@/shared/api/tauri";
import type { AgentProjectScope } from "@/shared/api/types";

export type ProjectConnectionHealthStatus =
  | "ready"
  | "not_tested"
  | "check_needed"
  | "sign_in_required"
  | "missing_access"
  | "unavailable";

export type ProjectConnectionHealth = {
  status: ProjectConnectionHealthStatus;
  lastVerifiedAt: string | null;
  detail: string | null;
};

export type ProjectConnection = {
  id: string;
  projectScope: AgentProjectScope;
  name: string;
  provider: string;
  /** Verified capabilities discovered by the connection test. */
  capabilityIds: string[];
  discoveredTools: string[];
  command: string;
  args: string[];
  /** Secret names only. Values are never returned by the backend. */
  envKeys: string[];
  health: ProjectConnectionHealth;
  createdAt: string;
  updatedAt: string;
  generation: number;
};

export type ProjectConnectionDraft = {
  projectScope: AgentProjectScope;
  name: string;
  provider: string;
  command: string;
  args: string[];
  /** Secret values are write-only and must never be echoed by the backend. */
  env: Record<string, string>;
  /** Existing secret names the user explicitly removed. */
  removeEnvKeys?: string[];
};

export type ProjectConnectionImpact = {
  connectionId: string;
  agents: Array<{ pubkey: string; name: string }>;
};

type RawProjectConnection = Omit<
  ProjectConnection,
  "args" | "capabilityIds" | "discoveredTools" | "envKeys"
> & {
  args?: string[];
  capabilityIds?: string[];
  discoveredTools?: string[];
  envKeys?: string[];
};

function fromRawProjectConnection(
  connection: RawProjectConnection,
): ProjectConnection {
  return {
    ...connection,
    args: connection.args ?? [],
    capabilityIds: connection.capabilityIds ?? [],
    discoveredTools: connection.discoveredTools ?? [],
    envKeys: connection.envKeys ?? [],
  };
}

export async function listProjectConnections(
  projectScope: AgentProjectScope,
): Promise<ProjectConnection[]> {
  return (
    await invokeTauri<RawProjectConnection[]>("list_project_connections", {
      projectScope,
    })
  ).map(fromRawProjectConnection);
}

export async function createProjectConnection(
  input: ProjectConnectionDraft,
): Promise<ProjectConnection> {
  return fromRawProjectConnection(
    await invokeTauri<RawProjectConnection>("create_project_connection", {
      input,
    }),
  );
}

export async function updateProjectConnection(
  input: ProjectConnectionDraft & { id: string },
): Promise<ProjectConnection> {
  return fromRawProjectConnection(
    await invokeTauri<RawProjectConnection>("update_project_connection", {
      input,
    }),
  );
}

export async function testProjectConnection(
  connectionId: string,
): Promise<ProjectConnection> {
  return fromRawProjectConnection(
    await invokeTauri<RawProjectConnection>("test_project_connection", {
      connectionId,
    }),
  );
}

export async function getProjectConnectionImpact(
  connectionId: string,
): Promise<ProjectConnectionImpact> {
  return invokeTauri<ProjectConnectionImpact>("get_project_connection_impact", {
    connectionId,
  });
}

export async function deleteProjectConnection(
  connectionId: string,
): Promise<void> {
  await invokeTauri("delete_project_connection", { connectionId });
}
