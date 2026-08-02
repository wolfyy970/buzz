import { Minus, Plus, RefreshCw } from "lucide-react";

import type {
  AgentTemplateSkillFileChanges,
  AgentTemplateSkillChanges,
  AgentTemplateUpdateTarget,
} from "@/shared/api/tauriAgentTemplateUpdates";

function SkillFileChangesDetails({
  changes,
}: {
  changes: AgentTemplateSkillFileChanges;
}) {
  const count =
    changes.added.length + changes.changed.length + changes.removed.length;
  if (count === 0) return null;

  return (
    <details className="ml-5 rounded-lg border border-border/70 bg-background/60 px-3 py-2">
      <summary className="cursor-pointer text-xs font-medium text-foreground">
        Review {count} {count === 1 ? "file change" : "file changes"}
      </summary>
      <div className="mt-3 space-y-3">
        {changes.added.map((file) => (
          <div key={`added:${file.path}`}>
            <p className="break-all text-2xs font-medium text-status-added">
              Added {file.path}
            </p>
            <pre className="mt-1 max-h-48 overflow-auto whitespace-pre-wrap rounded-md bg-muted/40 p-2 text-2xs text-foreground">
              {file.content}
            </pre>
          </div>
        ))}
        {changes.changed.map((file) => (
          <div className="space-y-2" key={`changed:${file.path}`}>
            <p className="break-all text-2xs font-medium text-status-modified">
              Changed {file.path}
            </p>
            <div>
              <p className="text-2xs font-medium text-muted-foreground">
                Current
              </p>
              <pre className="mt-1 max-h-48 overflow-auto whitespace-pre-wrap rounded-md bg-muted/40 p-2 text-2xs text-foreground">
                {file.before}
              </pre>
            </div>
            <div>
              <p className="text-2xs font-medium text-muted-foreground">
                New version
              </p>
              <pre className="mt-1 max-h-48 overflow-auto whitespace-pre-wrap rounded-md bg-muted/40 p-2 text-2xs text-foreground">
                {file.after}
              </pre>
            </div>
          </div>
        ))}
        {changes.removed.map((file) => (
          <div key={`removed:${file.path}`}>
            <p className="break-all text-2xs font-medium text-muted-foreground">
              Removed {file.path}
            </p>
            <pre className="mt-1 max-h-48 overflow-auto whitespace-pre-wrap rounded-md bg-muted/40 p-2 text-2xs text-foreground">
              {file.content}
            </pre>
          </div>
        ))}
      </div>
    </details>
  );
}

export function commonAgentSkillChanges(
  agents: readonly AgentTemplateUpdateTarget[],
): AgentTemplateSkillChanges | null {
  const first = agents[0]?.skillChanges;
  if (!first) return null;
  const serialized = JSON.stringify(first);
  return agents.every(
    (agent) => JSON.stringify(agent.skillChanges) === serialized,
  )
    ? first
    : null;
}

export function AgentTemplateSkillChangesSummary({
  changes,
}: {
  changes: AgentTemplateSkillChanges;
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
        Skills in this update
      </h3>
      <div className="space-y-1.5 text-xs">
        {changes.added.map((skill) => (
          <div className="flex items-center gap-2" key={`add:${skill.name}`}>
            <Plus className="h-3.5 w-3.5 text-status-added" />
            <span>
              Added <strong>{skill.name}</strong>
            </span>
          </div>
        ))}
        {changes.changed.map(({ after, before, fileChanges }) => (
          <div className="space-y-1.5" key={`change:${after.name}`}>
            <div className="flex items-center gap-2">
              <RefreshCw className="h-3.5 w-3.5 text-status-modified" />
              <span>
                Changed <strong>{before.name}</strong>
                {before.name !== after.name ? ` to ${after.name}` : ""}
              </span>
            </div>
            <SkillFileChangesDetails changes={fileChanges} />
          </div>
        ))}
        {changes.removed.map((skill) => (
          <div className="flex items-center gap-2" key={`remove:${skill.name}`}>
            <Minus className="h-3.5 w-3.5 text-muted-foreground" />
            <span>
              Removed <strong>{skill.name}</strong>
            </span>
          </div>
        ))}
      </div>
    </section>
  );
}
