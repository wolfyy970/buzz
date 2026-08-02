import type { ManagedAgent } from "@/shared/api/types";
import { cn } from "@/shared/lib/cn";

export function agentTemplateUsageLabel(count: number): string {
  if (count === 0) return "No agents use this template yet";
  return `Used by ${count} ${count === 1 ? "agent" : "agents"}`;
}

export function AgentTemplateImpactPreview({
  agents,
  className,
  surface = "card",
}: {
  agents: ManagedAgent[];
  className?: string;
  surface?: "card" | "plain";
}) {
  const visibleAgents = agents.slice(0, 6);
  const hiddenCount = agents.length - visibleAgents.length;

  return (
    <div
      className={cn(
        "block",
        surface === "card" &&
          "rounded-xl border border-border/60 bg-muted/20 px-4 py-3",
        className,
      )}
      data-testid="template-impact-preview"
    >
      <p className="text-sm font-medium text-foreground">
        {agentTemplateUsageLabel(agents.length)}
      </p>
      {agents.length > 0 ? (
        <ul
          aria-label="Agents using this template"
          className="mt-2 flex flex-wrap gap-1.5"
        >
          {visibleAgents.map((agent) => (
            <li
              className="max-w-full break-words rounded-full bg-muted px-2.5 py-1 text-xs text-muted-foreground"
              data-testid={`template-impact-agent-${agent.pubkey}`}
              key={agent.pubkey}
              title={agent.name}
            >
              {agent.name}
            </li>
          ))}
          {hiddenCount > 0 ? (
            <li className="rounded-full bg-muted px-2.5 py-1 text-xs text-muted-foreground">
              +{hiddenCount} more
            </li>
          ) : null}
        </ul>
      ) : null}
    </div>
  );
}
