import { AlertCircle, FolderGit2, Link2, LoaderCircle } from "lucide-react";
import * as React from "react";

import type { Project } from "@/features/projects/hooks";
import { useProjectConnectionsQuery } from "@/features/projects/projectConnectionHooks";
import type { AgentToolRequirement } from "@/shared/api/types";
import { PersonaDropdownField } from "./PersonaDropdownField";
import { resolveAgentProjectAccessReadiness } from "./agentProjectAccessPolicy";

export type AgentProjectAccessDraft = {
  /** Local UI identity only. Never sent to Tauri or persisted. */
  projectId: string;
  connectionBindings: Record<string, string>;
};

export type AgentProjectAccessReadiness = {
  ready: boolean;
  reason: string | null;
};

export const emptyAgentProjectAccessDraft: AgentProjectAccessDraft = {
  projectId: "",
  connectionBindings: {},
};

export function AgentProjectAccessSection({
  disabled,
  draft,
  onDraftChange,
  onReadinessChange,
  operatorPubkey,
  projects,
  projectsLoading,
  relayUrl,
  toolRequirements,
}: {
  disabled: boolean;
  draft: AgentProjectAccessDraft;
  onDraftChange: (draft: AgentProjectAccessDraft) => void;
  onReadinessChange: (readiness: AgentProjectAccessReadiness) => void;
  operatorPubkey: string | null;
  projects: readonly Project[];
  projectsLoading: boolean;
  relayUrl: string | null;
  toolRequirements: readonly AgentToolRequirement[];
}) {
  const selectedProject =
    projects.find((project) => project.id === draft.projectId) ?? null;
  const projectScope = React.useMemo(
    () =>
      selectedProject?.projectChannelId && relayUrl && operatorPubkey
        ? {
            relayUrl,
            operatorPubkey,
            repoAddress: selectedProject.repoAddress,
            channelId: selectedProject.projectChannelId,
          }
        : null,
    [operatorPubkey, relayUrl, selectedProject],
  );
  const connectionsQuery = useProjectConnectionsQuery(projectScope);
  const connections = React.useMemo(
    () => connectionsQuery.data ?? [],
    [connectionsQuery.data],
  );

  React.useEffect(() => {
    onReadinessChange(
      resolveAgentProjectAccessReadiness({
        connections,
        connectionsError: connectionsQuery.isError,
        connectionsPending: connectionsQuery.isPending,
        draft,
        scopeAvailable: Boolean(projectScope),
        selectedProject,
        toolRequirements,
      }),
    );
  }, [
    connections,
    connectionsQuery.isError,
    connectionsQuery.isPending,
    draft,
    onReadinessChange,
    projectScope,
    selectedProject,
    toolRequirements,
  ]);

  const projectOptions = projects.map((project) => ({
    disabled: !project.projectChannelId,
    label: project.projectChannelId
      ? project.name
      : `${project.name} (discussion channel needed)`,
    value: project.id,
  }));

  return (
    <section className="space-y-3" data-testid="agent-project-access-section">
      <div className="flex items-center gap-2">
        <FolderGit2 className="h-4 w-4 text-muted-foreground" />
        <h3 className="text-sm font-medium text-foreground">Project</h3>
      </div>
      <p className="text-xs text-muted-foreground">
        Choose where this agent works. Its connected tools and conversations
        stay inside this Project.
      </p>
      <div className="space-y-1.5">
        <label
          className="text-xs font-medium text-foreground"
          htmlFor="agent-project"
        >
          Project
        </label>
        <PersonaDropdownField
          disabled={disabled || projectsLoading}
          id="agent-project"
          onValueChange={(projectId) =>
            onDraftChange({ projectId, connectionBindings: {} })
          }
          options={projectOptions}
          placeholder={
            projectsLoading
              ? "Loading Projects..."
              : projectOptions.length === 0
                ? "No Projects available"
                : "Choose a Project"
          }
          value={draft.projectId}
        />
      </div>

      {selectedProject && toolRequirements.length > 0 ? (
        <div className="space-y-3 rounded-xl border border-border/70 bg-muted/10 p-3">
          <div className="flex items-center gap-2">
            <Link2 className="h-4 w-4 text-muted-foreground" />
            <p className="text-xs font-medium text-foreground">
              Connect this agent's tools
            </p>
          </div>
          {connectionsQuery.isPending ? (
            <div className="flex items-center gap-2 text-xs text-muted-foreground">
              <LoaderCircle className="h-4 w-4 animate-spin" />
              Loading connections...
            </div>
          ) : connectionsQuery.isError ? (
            <div className="flex items-start gap-2 text-xs text-destructive">
              <AlertCircle className="mt-0.5 h-4 w-4 shrink-0" />
              <span>
                Couldn't load connections. Your saved connections were not
                changed.
              </span>
            </div>
          ) : (
            toolRequirements.map((requirement) => {
              const compatible = connections.filter((connection) =>
                connection.capabilityIds.includes(requirement.capability),
              );
              const options = compatible.map((connection) => ({
                disabled: connection.health.status !== "ready",
                label:
                  connection.health.status === "ready"
                    ? connection.name
                    : `${connection.name} (${connection.health.status.replaceAll("_", " ")})`,
                value: connection.id,
              }));
              return (
                <div className="space-y-1.5" key={requirement.id}>
                  <label
                    className="text-xs font-medium text-foreground"
                    htmlFor={`agent-tool-binding-${requirement.id}`}
                  >
                    {requirement.label || "Unnamed tool"}
                    {!requirement.required ? (
                      <span className="ml-1 font-normal text-muted-foreground">
                        Optional
                      </span>
                    ) : null}
                  </label>
                  <PersonaDropdownField
                    disabled={disabled}
                    id={`agent-tool-binding-${requirement.id}`}
                    onValueChange={(connectionId) =>
                      onDraftChange({
                        ...draft,
                        connectionBindings: {
                          ...draft.connectionBindings,
                          [requirement.id]: connectionId,
                        },
                      })
                    }
                    options={options}
                    placeholder={
                      options.length > 0
                        ? "Choose a connection"
                        : `No connection can provide ${requirement.label || "this tool"}.`
                    }
                    value={draft.connectionBindings[requirement.id] ?? ""}
                  />
                  {options.length === 0 ? (
                    <p className="text-xs text-amber-700 dark:text-amber-300">
                      Open {selectedProject.name} → Connections to add and test
                      one.
                    </p>
                  ) : null}
                </div>
              );
            })
          )}
        </div>
      ) : null}
    </section>
  );
}
