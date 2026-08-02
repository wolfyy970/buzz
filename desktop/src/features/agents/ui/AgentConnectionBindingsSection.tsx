import { AlertCircle, Link2, LoaderCircle } from "lucide-react";
import * as React from "react";

import { useProjectsQuery } from "@/features/projects/hooks";
import { useProjectConnectionsQuery } from "@/features/projects/projectConnectionHooks";
import type {
  AgentProjectScope,
  AgentToolRequirement,
} from "@/shared/api/types";
import { PersonaDropdownField } from "./PersonaDropdownField";

export function AgentConnectionBindingsSection({
  bindings,
  disabled,
  onBindingsChange,
  onValidityChange,
  projectScope,
  requirements,
}: {
  bindings: Record<string, string>;
  disabled: boolean;
  onBindingsChange: (bindings: Record<string, string>) => void;
  onValidityChange: (valid: boolean) => void;
  projectScope: AgentProjectScope | null;
  requirements: readonly AgentToolRequirement[];
}) {
  const projectsQuery = useProjectsQuery();
  const project = projectsQuery.data?.find(
    (candidate) => candidate.repoAddress === projectScope?.repoAddress,
  );
  const connectionsQuery = useProjectConnectionsQuery(projectScope);
  const connections = connectionsQuery.data ?? [];

  React.useEffect(() => {
    const requiredRequirements = requirements.filter(
      (requirement) => requirement.required,
    );
    const valid =
      requiredRequirements.length === 0 ||
      (Boolean(projectScope) &&
        !connectionsQuery.isPending &&
        !connectionsQuery.isError &&
        requiredRequirements.every((requirement) => {
          const connection = connections.find(
            (candidate) => candidate.id === bindings[requirement.id],
          );
          return (
            connection?.health.status === "ready" &&
            connection.capabilityIds.includes(requirement.capability)
          );
        }));
    onValidityChange(valid);
  }, [
    bindings,
    connections,
    connectionsQuery.isError,
    connectionsQuery.isPending,
    onValidityChange,
    projectScope,
    requirements,
  ]);

  if (requirements.length === 0) return null;

  return (
    <section className="space-y-3" data-testid="agent-connection-bindings">
      <div className="flex items-center gap-2">
        <Link2 className="h-4 w-4 text-muted-foreground" />
        <h3 className="text-sm font-medium text-foreground">Connections</h3>
      </div>
      <p className="text-xs text-muted-foreground">
        {project
          ? `Choose which ${project.name} connections this agent uses.`
          : "Choose which Project connections this agent uses."}
      </p>
      {!projectScope ? (
        <div className="flex items-start gap-2 rounded-xl border border-amber-500/30 bg-amber-500/10 p-3 text-xs text-amber-700 dark:text-amber-300">
          <AlertCircle className="mt-0.5 h-4 w-4 shrink-0" />
          This legacy agent has no Project. Create a new agent from its template
          to connect these tools.
        </div>
      ) : connectionsQuery.isPending ? (
        <div className="flex items-center gap-2 text-xs text-muted-foreground">
          <LoaderCircle className="h-4 w-4 animate-spin" />
          Loading connections...
        </div>
      ) : connectionsQuery.isError ? (
        <div className="flex items-start gap-2 text-xs text-destructive">
          <AlertCircle className="mt-0.5 h-4 w-4 shrink-0" />
          Couldn't load connections. Your saved connections were not changed.
        </div>
      ) : (
        requirements.map((requirement) => {
          const compatible = connections.filter((connection) =>
            connection.capabilityIds.includes(requirement.capability),
          );
          return (
            <div className="space-y-1.5" key={requirement.id}>
              <label
                className="text-xs font-medium text-foreground"
                htmlFor={`edit-agent-tool-binding-${requirement.id}`}
              >
                {requirement.label}
                {!requirement.required ? (
                  <span className="ml-1 font-normal text-muted-foreground">
                    Optional
                  </span>
                ) : null}
              </label>
              <PersonaDropdownField
                disabled={disabled}
                id={`edit-agent-tool-binding-${requirement.id}`}
                onValueChange={(connectionId) =>
                  onBindingsChange({
                    ...bindings,
                    [requirement.id]: connectionId,
                  })
                }
                options={compatible.map((connection) => ({
                  disabled: connection.health.status !== "ready",
                  label:
                    connection.health.status === "ready"
                      ? connection.name
                      : `${connection.name} (${connection.health.status.replaceAll("_", " ")})`,
                  value: connection.id,
                }))}
                placeholder={
                  compatible.length > 0
                    ? "Choose a connection"
                    : `No connection can provide ${requirement.label}.`
                }
                value={bindings[requirement.id] ?? ""}
              />
            </div>
          );
        })
      )}
    </section>
  );
}
