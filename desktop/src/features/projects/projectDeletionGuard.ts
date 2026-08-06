import type { ProjectConnectionScope } from "@/shared/api/projectConnectionTypes";

export class ProjectHasConnectionsError extends Error {
  readonly connectionCount: number;

  constructor(connectionCount: number) {
    super(
      `Remove ${connectionCount === 1 ? "this Project's connection" : `this Project's ${connectionCount} connections`} before deleting it.`,
    );
    this.name = "ProjectHasConnectionsError";
    this.connectionCount = connectionCount;
  }
}

/**
 * Fails closed when a Project still owns device-local Connections.
 *
 * Project deletion is shared relay state. Connection credentials are local
 * state, so publishing the deletion must never implicitly remove them.
 */
export async function assertProjectHasNoConnections(
  projectScope: ProjectConnectionScope,
  listConnections: (
    scope: ProjectConnectionScope,
  ) => Promise<readonly unknown[]>,
): Promise<void> {
  const connections = await listConnections(projectScope);
  if (connections.length > 0) {
    throw new ProjectHasConnectionsError(connections.length);
  }
}
