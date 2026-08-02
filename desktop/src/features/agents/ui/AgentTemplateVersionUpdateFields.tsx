import {
  ArrowRight,
  KeyRound,
  ShieldCheck,
  SlidersHorizontal,
} from "lucide-react";

import type {
  AgentTemplateOverridesPreserved,
  AgentTemplateUpdateTarget,
  AgentTemplateVersionChanges,
} from "@/shared/api/tauriAgentTemplateUpdates";
import type { RespondToMode } from "@/shared/api/types";

function accessLabel(mode: RespondToMode) {
  switch (mode) {
    case "owner-only":
      return "Owner only";
    case "allowlist":
      return "Selected people";
    case "anyone":
      return "Anyone";
  }
}

function optionalValue(value: string | null, fallback: string) {
  return value?.trim() || fallback;
}

function ChangeRow({
  after,
  before,
  label,
}: {
  after: string;
  before: string;
  label: string;
}) {
  return (
    <div className="grid grid-cols-[7rem_minmax(0,1fr)] items-start gap-3 py-1.5 text-xs">
      <dt className="font-medium text-muted-foreground">{label}</dt>
      <dd className="flex min-w-0 items-center gap-2 text-foreground">
        <span className="min-w-0 truncate" title={before}>
          {before}
        </span>
        <ArrowRight className="size-3.5 shrink-0 text-muted-foreground" />
        <span className="min-w-0 truncate font-medium" title={after}>
          {after}
        </span>
      </dd>
    </div>
  );
}

export function commonAgentVersionChanges(
  agents: readonly AgentTemplateUpdateTarget[],
): AgentTemplateVersionChanges | null {
  const first = agents[0]?.versionChanges;
  if (!first) return null;
  const serialized = JSON.stringify(first);
  return agents.every(
    (agent) => JSON.stringify(agent.versionChanges) === serialized,
  )
    ? first
    : null;
}

export function hasAgentVersionChanges(changes: AgentTemplateVersionChanges) {
  return Boolean(
    changes.runtime ||
      changes.provider ||
      changes.model ||
      changes.access ||
      changes.parallelism ||
      changes.environment.addedKeys.length > 0 ||
      changes.environment.changedKeys.length > 0 ||
      changes.environment.removedKeys.length > 0,
  );
}

export function AgentTemplateVersionChangesSummary({
  changes,
}: {
  changes: AgentTemplateVersionChanges;
}) {
  if (!hasAgentVersionChanges(changes)) return null;

  const environmentChanges = [
    ...changes.environment.addedKeys.map((key) => `${key} added`),
    ...changes.environment.changedKeys.map((key) => `${key} changed`),
    ...changes.environment.removedKeys.map((key) => `${key} removed`),
  ];

  return (
    <section className="rounded-xl border border-border bg-muted/20 p-4">
      <div className="flex items-center gap-2">
        <SlidersHorizontal className="size-4 text-muted-foreground" />
        <h3 className="text-sm font-medium text-foreground">
          Configuration in this update
        </h3>
      </div>
      <dl className="mt-2 divide-y divide-border/60">
        {changes.runtime ? (
          <ChangeRow
            after={optionalValue(changes.runtime.after, "Default runtime")}
            before={optionalValue(changes.runtime.before, "Default runtime")}
            label="Runtime"
          />
        ) : null}
        {changes.provider ? (
          <ChangeRow
            after={optionalValue(changes.provider.after, "Inherited")}
            before={optionalValue(changes.provider.before, "Inherited")}
            label="Provider"
          />
        ) : null}
        {changes.model ? (
          <ChangeRow
            after={optionalValue(changes.model.after, "Automatic")}
            before={optionalValue(changes.model.before, "Automatic")}
            label="Model"
          />
        ) : null}
        {changes.access ? (
          <>
            <ChangeRow
              after={accessLabel(changes.access.after)}
              before={accessLabel(changes.access.before)}
              label="Access"
            />
            {changes.access.allowlistAdded.length > 0 ||
            changes.access.allowlistRemoved.length > 0 ? (
              <div className="grid grid-cols-[7rem_minmax(0,1fr)] gap-3 py-1.5 text-xs">
                <dt className="font-medium text-muted-foreground">
                  Access list
                </dt>
                <dd className="text-foreground">
                  {[
                    changes.access.allowlistAdded.length > 0
                      ? `${changes.access.allowlistAdded.length} added`
                      : null,
                    changes.access.allowlistRemoved.length > 0
                      ? `${changes.access.allowlistRemoved.length} removed`
                      : null,
                  ]
                    .filter(Boolean)
                    .join(", ")}
                </dd>
              </div>
            ) : null}
          </>
        ) : null}
        {changes.parallelism ? (
          <ChangeRow
            after={String(changes.parallelism.after)}
            before={String(changes.parallelism.before)}
            label="Parallel work"
          />
        ) : null}
        {environmentChanges.length > 0 ? (
          <div className="grid grid-cols-[7rem_minmax(0,1fr)] gap-3 py-1.5 text-xs">
            <dt className="font-medium text-muted-foreground">Environment</dt>
            <dd className="min-w-0">
              <div className="flex items-start gap-1.5 text-foreground">
                <KeyRound className="mt-0.5 size-3.5 shrink-0 text-muted-foreground" />
                <span className="break-words">
                  {environmentChanges.join(", ")}
                </span>
              </div>
              <p className="mt-1 text-2xs text-muted-foreground">
                Values stay hidden.
              </p>
            </dd>
          </div>
        ) : null}
      </dl>
    </section>
  );
}

export function AgentTemplateOverridesPreservedSummary({
  overrides,
}: {
  overrides: AgentTemplateOverridesPreserved;
}) {
  const labels = [
    overrides.instructions ? "instructions" : null,
    overrides.runtime ? "runtime" : null,
    overrides.provider ? "provider" : null,
    overrides.model ? "model" : null,
    overrides.skills ? "Skills" : null,
    overrides.localEnvironment ? "local environment" : null,
  ].filter((label): label is string => label !== null);

  if (labels.length === 0) return null;

  return (
    <div
      className="mt-3 flex items-start gap-2 rounded-lg bg-muted/40 px-3 py-2 text-xs"
      data-testid="template-update-preserved-overrides"
    >
      <ShieldCheck className="mt-0.5 size-3.5 shrink-0 text-muted-foreground" />
      <div className="min-w-0">
        <p className="font-medium text-foreground">
          This agent keeps its own {labels.join(", ")}.
        </p>
        {overrides.localEnvironmentKeys.length > 0 ? (
          <p className="mt-1 break-words text-muted-foreground">
            Local keys: {overrides.localEnvironmentKeys.join(", ")}
          </p>
        ) : null}
      </div>
    </div>
  );
}
