import { Plus, Trash2, Wrench } from "lucide-react";

import type { AgentToolRequirement } from "@/shared/api/types";
import { Button } from "@/shared/ui/button";
import { Checkbox } from "@/shared/ui/checkbox";
import { Input } from "@/shared/ui/input";
import type { AgentToolRequirementIssue } from "./agentToolRequirements";
export {
  agentToolRequirementIssues,
  agentToolRequirementsValid,
} from "./agentToolRequirements";

function newRequirementId() {
  return `tool_${crypto.randomUUID().replaceAll("-", "")}`;
}

export function AgentToolsSection({
  disabled,
  issues = [],
  onChange,
  value,
}: {
  disabled: boolean;
  issues?: AgentToolRequirementIssue[];
  onChange: (value: AgentToolRequirement[]) => void;
  value: AgentToolRequirement[];
}) {
  function update(
    id: string,
    patch: Partial<Omit<AgentToolRequirement, "id">>,
  ) {
    onChange(
      value.map((requirement) =>
        requirement.id === id ? { ...requirement, ...patch } : requirement,
      ),
    );
  }

  function issueFor(index: number, field: AgentToolRequirementIssue["field"]) {
    return issues.find(
      (issue) => issue.index === index && issue.field === field,
    );
  }

  return (
    <section className="space-y-3" data-testid="agent-tools-section">
      <div className="flex items-start justify-between gap-3">
        <div>
          <div className="flex items-center gap-2">
            <Wrench className="h-4 w-4 text-muted-foreground" />
            <h3 className="text-base font-semibold text-foreground">Tools</h3>
          </div>
          <p className="mt-1 text-xs text-muted-foreground">
            Capabilities this template expects. Each agent connects them from
            its project.
          </p>
        </div>
        <Button
          className="min-h-9"
          disabled={disabled}
          onClick={() =>
            onChange([
              ...value,
              {
                id: newRequirementId(),
                label: "",
                capability: "",
                required: true,
              },
            ])
          }
          size="xs"
          type="button"
          variant="outline"
        >
          <Plus className="h-3.5 w-3.5" />
          Add tool
        </Button>
      </div>

      {value.length === 0 ? (
        <div className="rounded-xl border border-dashed border-border/70 px-4 py-3 text-xs text-muted-foreground">
          This template does not require any connected tools.
        </div>
      ) : (
        <div className="space-y-3">
          {value.map((requirement, index) => (
            <div
              className="space-y-3 rounded-xl border border-border/70 bg-muted/10 p-3"
              key={requirement.id}
            >
              {issueFor(index, "row") ? (
                <p className="text-xs text-destructive" role="alert">
                  {issueFor(index, "row")?.message}
                </p>
              ) : null}
              <div className="flex items-start gap-2">
                <div className="min-w-0 flex-1 space-y-1.5">
                  <label
                    className="text-xs font-medium text-foreground"
                    htmlFor={`agent-tool-label-${requirement.id}`}
                  >
                    Tool name
                  </label>
                  <Input
                    aria-describedby={
                      issueFor(index, "label")
                        ? `agent-tool-label-error-${requirement.id}`
                        : undefined
                    }
                    aria-invalid={Boolean(issueFor(index, "label"))}
                    disabled={disabled}
                    id={`agent-tool-label-${requirement.id}`}
                    onChange={(event) =>
                      update(requirement.id, { label: event.target.value })
                    }
                    placeholder="Analytics reports"
                    value={requirement.label}
                  />
                  {issueFor(index, "label") ? (
                    <p
                      className="text-xs text-destructive"
                      id={`agent-tool-label-error-${requirement.id}`}
                      role="alert"
                    >
                      {issueFor(index, "label")?.message}
                    </p>
                  ) : null}
                </div>
                <Button
                  aria-label={`Remove tool ${index + 1}`}
                  disabled={disabled}
                  onClick={() =>
                    onChange(value.filter((item) => item.id !== requirement.id))
                  }
                  className="mt-6 size-9"
                  size="icon"
                  type="button"
                  variant="ghost"
                >
                  <Trash2 className="h-4 w-4" />
                </Button>
              </div>
              <div className="space-y-1.5">
                <label
                  className="text-xs font-medium text-foreground"
                  htmlFor={`agent-tool-capability-${requirement.id}`}
                >
                  Capability ID
                </label>
                <Input
                  aria-describedby={
                    issueFor(index, "capability")
                      ? `agent-tool-capability-error-${requirement.id}`
                      : `agent-tool-capability-help-${requirement.id}`
                  }
                  aria-invalid={Boolean(issueFor(index, "capability"))}
                  autoCapitalize="off"
                  autoCorrect="off"
                  className="font-mono text-xs"
                  disabled={disabled}
                  id={`agent-tool-capability-${requirement.id}`}
                  onChange={(event) =>
                    update(requirement.id, {
                      capability: event.target.value.trim(),
                    })
                  }
                  placeholder="mcp.tool.run_report"
                  spellCheck={false}
                  value={requirement.capability}
                />
                <p
                  className="text-xs text-muted-foreground"
                  id={`agent-tool-capability-help-${requirement.id}`}
                >
                  Use the ID shown after testing a compatible Project
                  connection.
                </p>
                {issueFor(index, "capability") ? (
                  <p
                    className="text-xs text-destructive"
                    id={`agent-tool-capability-error-${requirement.id}`}
                    role="alert"
                  >
                    {issueFor(index, "capability")?.message}
                  </p>
                ) : null}
              </div>
              <label
                className="flex min-h-9 items-center gap-2 text-xs text-foreground"
                htmlFor={`agent-tool-required-${requirement.id}`}
              >
                <Checkbox
                  checked={requirement.required}
                  disabled={disabled}
                  id={`agent-tool-required-${requirement.id}`}
                  onCheckedChange={(checked) =>
                    update(requirement.id, { required: checked === true })
                  }
                />
                Required to launch
              </label>
            </div>
          ))}
        </div>
      )}
    </section>
  );
}
