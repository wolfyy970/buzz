import * as React from "react";

import { useCommunities } from "@/features/communities/useCommunities";
import { useProjectsQuery } from "@/features/projects/hooks";
import {
  agentProjectScopeAddress,
  projectConnectionScope,
  projectMatchesConnectionAddress,
} from "@/features/projects/projectConnectionScope";
import { useIdentityQuery } from "@/shared/api/hooks";
import type { AgentProjectScope, ManagedAgent } from "@/shared/api/types";
import {
  AgentProjectAccessSection,
  type AgentProjectAccessDraft,
  type AgentProjectAccessReadiness,
} from "./AgentProjectAccessSection";

function recordsEqual(
  left: Record<string, string>,
  right: Record<string, string>,
) {
  const leftEntries = Object.entries(left);
  return (
    leftEntries.length === Object.keys(right).length &&
    leftEntries.every(([key, value]) => right[key] === value)
  );
}

function scopesEqual(
  left: AgentProjectScope | null,
  right: AgentProjectScope | null,
) {
  return (
    left?.relayUrl === right?.relayUrl &&
    left?.operatorPubkey === right?.operatorPubkey &&
    agentProjectScopeAddress(left) === agentProjectScopeAddress(right) &&
    left?.channelId === right?.channelId
  );
}

export function useAgentProjectAccessDraft({
  agent,
  disabled,
  open,
}: {
  agent: ManagedAgent;
  disabled: boolean;
  open: boolean;
}) {
  const projectsQuery = useProjectsQuery();
  const { activeCommunity } = useCommunities();
  const identityQuery = useIdentityQuery();
  const userTouched = React.useRef(false);
  const initialDraft = React.useCallback(
    (): AgentProjectAccessDraft => ({
      projectId:
        agent.projectScope == null
          ? ""
          : (projectsQuery.data?.find((project) =>
              projectMatchesConnectionAddress(
                project,
                agentProjectScopeAddress(agent.projectScope),
              ),
            )?.id ?? ""),
      connectionBindings: agent.connectionBindings,
    }),
    [agent, projectsQuery.data],
  );
  const [draft, setDraft] =
    React.useState<AgentProjectAccessDraft>(initialDraft);
  const [readiness, setReadiness] = React.useState<AgentProjectAccessReadiness>(
    {
      ready: !agent.toolRequirements.some(
        (requirement) => requirement.required,
      ),
      reason: null,
    },
  );

  // Polling must not wipe in-dialog edits. Reset only when opening the dialog
  // or switching to a different agent.
  // biome-ignore lint/correctness/useExhaustiveDependencies: the explicit lifecycle boundary is [open, agent.pubkey]
  React.useEffect(() => {
    if (!open) return;
    userTouched.current = false;
    setDraft(initialDraft());
    setReadiness({
      ready: !agent.toolRequirements.some(
        (requirement) => requirement.required,
      ),
      reason: null,
    });
  }, [agent.pubkey, open]);

  // Projects may finish loading after the dialog opens. Resolve the saved
  // Project once, but never over an in-progress user selection.
  React.useEffect(() => {
    if (!open || userTouched.current || !agent.projectScope) return;
    const projectId =
      projectsQuery.data?.find((project) =>
        projectMatchesConnectionAddress(
          project,
          agentProjectScopeAddress(agent.projectScope),
        ),
      )?.id ?? "";
    if (!projectId) return;
    setDraft((current) =>
      current.projectId === projectId ? current : { ...current, projectId },
    );
  }, [agent.projectScope, open, projectsQuery.data]);

  const handleDraftChange = React.useCallback(
    (nextDraft: AgentProjectAccessDraft) => {
      userTouched.current = true;
      setDraft(nextDraft);
    },
    [],
  );
  const handleReadinessChange = React.useCallback(
    (nextReadiness: AgentProjectAccessReadiness) => {
      setReadiness((current) =>
        current.ready === nextReadiness.ready &&
        current.reason === nextReadiness.reason
          ? current
          : nextReadiness,
      );
    },
    [],
  );

  if (agent.toolRequirements.length === 0) {
    return {
      connectionBindingsUpdate: undefined,
      projectScopeUpdate: undefined,
      submitBlockReason: null,
      valid: true,
      section: null,
    };
  }

  const selectedProject =
    projectsQuery.data?.find((project) => project.id === draft.projectId) ??
    null;
  const selectedScope = projectConnectionScope({
    operatorPubkey: identityQuery.data?.pubkey ?? null,
    project: selectedProject,
    relayUrl: activeCommunity?.relayUrl ?? null,
  });

  return {
    connectionBindingsUpdate: recordsEqual(
      draft.connectionBindings,
      agent.connectionBindings,
    )
      ? undefined
      : draft.connectionBindings,
    projectScopeUpdate:
      selectedScope && !scopesEqual(selectedScope, agent.projectScope)
        ? selectedScope
        : undefined,
    submitBlockReason: readiness.ready ? null : readiness.reason,
    valid: readiness.ready,
    section: (
      <AgentProjectAccessSection
        disabled={disabled}
        draft={draft}
        onDraftChange={handleDraftChange}
        onReadinessChange={handleReadinessChange}
        operatorPubkey={identityQuery.data?.pubkey ?? null}
        projects={projectsQuery.data ?? []}
        projectsLoading={projectsQuery.isPending}
        relayUrl={activeCommunity?.relayUrl ?? null}
        toolRequirements={agent.toolRequirements}
      />
    ),
  };
}
