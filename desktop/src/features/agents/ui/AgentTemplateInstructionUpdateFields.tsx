import type {
  AgentTemplateInstructionChange,
  AgentTemplateUpdateTarget,
} from "@/shared/api/tauriAgentTemplateUpdates";

export function commonAgentInstructionChange(
  agents: readonly AgentTemplateUpdateTarget[],
): AgentTemplateInstructionChange | null {
  const first = agents[0]?.instructionChange;
  if (!first) return null;
  return agents.every(
    (agent) =>
      agent.instructionChange?.before === first.before &&
      agent.instructionChange.after === first.after &&
      agent.instructionChange.privateOverridePreserved ===
        first.privateOverridePreserved,
  )
    ? first
    : null;
}

function LiteralInstructions({
  label,
  value,
}: {
  label: string;
  value: string;
}) {
  return (
    <div className="min-w-0 space-y-1.5">
      <p className="text-xs font-medium text-muted-foreground">{label}</p>
      {value.length === 0 ? (
        <p className="rounded-lg border border-border bg-background/70 p-3 text-xs text-muted-foreground">
          No instructions
        </p>
      ) : (
        <pre className="max-h-48 overflow-auto whitespace-pre-wrap break-words rounded-lg border border-border bg-background/70 p-3 font-mono text-xs text-foreground">
          {value}
        </pre>
      )}
    </div>
  );
}

export function AgentTemplateInstructionChangesSummary({
  change,
}: {
  change: AgentTemplateInstructionChange;
}) {
  return (
    <section
      className="space-y-3 rounded-xl border border-border bg-muted/20 p-4"
      data-testid="template-instruction-changes"
    >
      <div className="space-y-1">
        <h3 className="text-sm font-medium text-foreground">
          Agent instructions in this update
        </h3>
        {change.privateOverridePreserved ? (
          <p className="text-xs text-muted-foreground">
            This agent keeps its private instructions. The new template
            instructions will be used if you reset it later.
          </p>
        ) : null}
      </div>
      <div className="grid gap-3 sm:grid-cols-2">
        <LiteralInstructions label="Current" value={change.before} />
        <LiteralInstructions label="New version" value={change.after} />
      </div>
    </section>
  );
}
