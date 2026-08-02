import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";

import {
  createProjectConnection,
  deleteProjectConnection,
  getProjectConnectionImpact,
  listProjectConnections,
  testProjectConnection,
  type ProjectConnectionDraft,
  updateProjectConnection,
} from "@/shared/api/tauriProjectConnections";
import type { AgentProjectScope } from "@/shared/api/types";

export const projectConnectionsQueryKey = (
  projectScope: AgentProjectScope | null,
) =>
  [
    "project-connections",
    projectScope?.relayUrl ?? "",
    projectScope?.operatorPubkey ?? "",
    projectScope?.repoAddress ?? "",
    projectScope?.channelId ?? "",
  ] as const;

export function useProjectConnectionsQuery(
  projectScope: AgentProjectScope | null,
  options?: { enabled?: boolean },
) {
  return useQuery({
    enabled: Boolean(projectScope) && (options?.enabled ?? true),
    queryKey: projectConnectionsQueryKey(projectScope),
    queryFn: () => listProjectConnections(projectScope as AgentProjectScope),
  });
}

export function useCreateProjectConnectionMutation(
  projectScope: AgentProjectScope,
) {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (input: ProjectConnectionDraft) =>
      createProjectConnection(input),
    onSuccess: async () => {
      await queryClient.invalidateQueries({
        queryKey: projectConnectionsQueryKey(projectScope),
      });
    },
  });
}

export function useUpdateProjectConnectionMutation(
  projectScope: AgentProjectScope,
) {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (input: ProjectConnectionDraft & { id: string }) =>
      updateProjectConnection(input),
    onSuccess: async () => {
      await queryClient.invalidateQueries({
        queryKey: projectConnectionsQueryKey(projectScope),
      });
    },
  });
}

export function useTestProjectConnectionMutation(
  projectScope: AgentProjectScope,
) {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: testProjectConnection,
    onSuccess: async () => {
      await queryClient.invalidateQueries({
        queryKey: projectConnectionsQueryKey(projectScope),
      });
    },
  });
}

export function useProjectConnectionImpactMutation() {
  return useMutation({ mutationFn: getProjectConnectionImpact });
}

export function useDeleteProjectConnectionMutation(
  projectScope: AgentProjectScope,
) {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: deleteProjectConnection,
    onSuccess: async () => {
      await queryClient.invalidateQueries({
        queryKey: projectConnectionsQueryKey(projectScope),
      });
    },
  });
}
