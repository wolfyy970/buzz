import { AlertCircle, Link2, Minus, Plus, RefreshCw } from "lucide-react";
import * as React from "react";

import { useProjectConnectionsQuery } from "@/features/projects/projectConnectionHooks";
import type {
  AgentTemplateToolChanges,
  AgentTemplateUpdateTarget,
} from "@/shared/api/tauriAgentTemplateUpdates";
import type { AgentToolRequirement } from "@/shared/api/types";
import { PersonaDropdownField } from "./PersonaDropdownField";

export function updatedToolRequirements(
  changes: AgentTemplateToolChanges,
): AgentToolRequirement[] {
  return [...changes.added, ...changes.changed.map((change) => change.after)];
}

export function bindingsForToolRequirements(
  bindings: Readonly<Record<string, string>>,
  requirements: readonly AgentToolRequirement[],
): Record<string, string> {
  return Object.fromEntries(
    requirements.flatMap((requirement) => {
      const connectionId = bindings[requirement.id];
      return connectionId ? [[requirement.id, connectionId]] : [];
    }),
  );
}

export function commonAgentToolChanges(
  agents: readonly AgentTemplateUpdateTarget[],
): AgentTemplateToolChanges | null {
  const first = agents[0]?.toolChanges;
  if (!first) return null;
  const serialized = JSON.stringify(first);
  return agents.every(
    (agent) => JSON.stringify(agent.toolChanges) === serialized,
  )
    ? first
    : null;
}

export function AgentTemplateToolChangesSummary({
  changes,
}: {
  changes: AgentTemplateToolChanges;
}) {
  if (
    changes.added.length === 0 &&
    changes.changed.length === 0 &&
    changes.removed.length === 0
  )
    return null;

  return (
    <section className="space-y-2 rounded-xl border border-border bg-muted/20 p-4">
      <h3 className="text-sm font-medium text-foreground">
        Tools in this update
      </h3>
      <div className="space-y-1.5 text-xs">
        {changes.added.map((requirement) => (
          <div
            className="flex items-center gap-2"
            key={`add:${requirement.id}`}
          >
            <Plus className="h-3.5 w-3.5 text-emerald-600" />
            <span>
              Added <strong>{requirement.label}</strong>
            </span>
          </div>
        ))}
        {changes.changed.map(({ after, before }) => (
          <div className="flex items-center gap-2" key={`change:${after.id}`}>
            <RefreshCw className="h-3.5 w-3.5 text-amber-600" />
            <span>
              Changed <strong>{before.label}</strong>
              {before.label !== after.label ? ` to ${after.label}` : ""}
            </span>
          </div>
        ))}
        {changes.removed.map((requirement) => (
          <div
            className="flex items-center gap-2"
            key={`remove:${requirement.id}`}
          >
            <Minus className="h-3.5 w-3.5 text-muted-foreground" />
            <span>
              Removed <strong>{requirement.label}</strong>
            </span>
          </div>
        ))}
      </div>
    </section>
  );
}

export function AgentTemplateToolBindings({
  agent,
  bindings,
  disabled,
  onBindingsChange,
  onValidityChange,
  requirements,
}: {
  agent: AgentTemplateUpdateTarget;
  bindings: Record<string, string>;
  disabled: boolean;
  onBindingsChange: (bindings: Record<string, string>) => void;
  onValidityChange: (valid: boolean) => void;
  requirements: readonly AgentToolRequirement[];
}) {
  const query = useProjectConnectionsQuery(agent.projectScope);
  const connections = React.useMemo(() => query.data ?? [], [query.data]);

  React.useEffect(() => {
    const valid =
      requirements.every((requirement) => {
        const connectionId = bindings[requirement.id];
        if (!connectionId) return !requirement.required;
        const connection = connections.find(
          (candidate) => candidate.id === connectionId,
        );
        return (
          connection?.health.status === "ready" &&
          connection.capabilityIds.includes(requirement.capability)
        );
      }) &&
      Object.keys(bindings).every((id) =>
        requirements.some((requirement) => requirement.id === id),
      ) &&
      (requirements.length === 0 ||
        (Boolean(agent.projectScope) && !query.isPending && !query.isError));
    onValidityChange(valid);
  }, [
    agent.projectScope,
    bindings,
    connections,
    onValidityChange,
    query.isError,
    query.isPending,
    requirements,
  ]);

  if (requirements.length === 0) return null;

  return (
    <div className="mt-3 space-y-3 rounded-lg border border-border/70 bg-background/60 p-3">
      <div className="flex items-center gap-2">
        <Link2 className="h-3.5 w-3.5 text-muted-foreground" />
        <p className="text-xs font-medium text-foreground">
          Connections for this update
        </p>
      </div>
      {!agent.projectScope ? (
        <div className="flex items-start gap-2 text-xs text-destructive">
          <AlertCircle className="mt-0.5 h-3.5 w-3.5 shrink-0" />
          This agent has no Project. Create a new agent from the updated
          template.
        </div>
      ) : query.isPending ? (
        <p className="text-xs text-muted-foreground">Loading connections...</p>
      ) : query.isError ? (
        <p className="text-xs text-destructive">
          Couldn't load this Project's connections. Nothing will be updated.
        </p>
      ) : (
        requirements.map((requirement) => {
          const compatible = connections.filter((connection) =>
            connection.capabilityIds.includes(requirement.capability),
          );
          return (
            <div className="space-y-1.5" key={requirement.id}>
              <label
                className="text-xs font-medium text-foreground"
                htmlFor={`template-update-binding-${agent.pubkey}-${requirement.id}`}
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
                id={`template-update-binding-${agent.pubkey}-${requirement.id}`}
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
    </div>
  );
}
