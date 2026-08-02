import { Minus, Plus, RefreshCw } from "lucide-react";

import type {
  AgentTemplateSkillChanges,
  AgentTemplateUpdateTarget,
} from "@/shared/api/tauriAgentTemplateUpdates";

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
            <Plus className="h-3.5 w-3.5 text-emerald-600" />
            <span>
              Added <strong>{skill.name}</strong>
            </span>
          </div>
        ))}
        {changes.changed.map(({ after, before }) => (
          <div className="flex items-center gap-2" key={`change:${after.name}`}>
            <RefreshCw className="h-3.5 w-3.5 text-amber-600" />
            <span>
              Changed <strong>{before.name}</strong>
              {before.name !== after.name ? ` to ${after.name}` : ""}
            </span>
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
