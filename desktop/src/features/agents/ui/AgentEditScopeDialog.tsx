import { Bot, ChevronRight, PanelsTopLeft } from "lucide-react";

import type { AgentPersona, ManagedAgent } from "@/shared/api/types";
import { ChooserDialogContent } from "@/shared/ui/chooser-dialog-content";
import { Dialog } from "@/shared/ui/dialog";
import { AgentTemplateImpactPreview } from "./AgentTemplateImpactPreview";

export function AgentEditScopeDialog({
  affectedAgents,
  agent,
  onEditInstance,
  onEditTemplate,
  onOpenChange,
  open,
  persona,
}: {
  affectedAgents: ManagedAgent[];
  agent: ManagedAgent;
  onEditInstance: () => void;
  onEditTemplate: () => void;
  onOpenChange: (open: boolean) => void;
  open: boolean;
  persona: AgentPersona;
}) {
  return (
    <Dialog onOpenChange={onOpenChange} open={open}>
      <ChooserDialogContent
        aria-describedby="agent-edit-scope-description"
        className="max-w-lg"
        contentClassName="space-y-3 pt-2"
        data-testid="agent-edit-scope-dialog"
        headerClassName="pb-2"
        title={`Edit ${agent.name}`}
      >
        <p
          className="text-sm leading-6 text-muted-foreground"
          id="agent-edit-scope-description"
        >
          Choose whether this change belongs to this agent or its template.
        </p>

        <button
          className="group flex w-full items-center gap-4 rounded-xl border border-border/70 bg-background px-4 py-4 text-left transition-colors hover:bg-muted/40 focus-visible:outline-hidden focus-visible:ring-1 focus-visible:ring-ring"
          data-testid="agent-edit-scope-instance"
          onClick={onEditInstance}
          type="button"
        >
          <span className="flex h-10 w-10 shrink-0 items-center justify-center rounded-lg bg-muted text-foreground">
            <Bot aria-hidden="true" className="h-5 w-5" />
          </span>
          <span className="min-w-0 flex-1">
            <span className="block text-sm font-semibold text-foreground">
              Edit this agent
            </span>
            <span className="mt-0.5 block text-sm text-muted-foreground">
              Only {agent.name} will change.
            </span>
          </span>
          <ChevronRight
            aria-hidden="true"
            className="h-4 w-4 shrink-0 text-muted-foreground transition-transform group-hover:translate-x-0.5"
          />
        </button>

        <div className="group rounded-xl border border-border/70 bg-background transition-colors hover:bg-muted/40">
          <button
            className="flex w-full items-start gap-4 px-4 pb-2 pt-4 text-left focus-visible:outline-hidden focus-visible:ring-1 focus-visible:ring-ring"
            data-testid="agent-edit-scope-template"
            onClick={onEditTemplate}
            type="button"
          >
            <span className="flex h-10 w-10 shrink-0 items-center justify-center rounded-lg bg-muted text-foreground">
              <PanelsTopLeft aria-hidden="true" className="h-5 w-5" />
            </span>
            <span className="min-w-0 flex-1">
              <span className="block text-sm font-semibold text-foreground">
                Edit its template
              </span>
              <span className="mt-0.5 block text-sm text-muted-foreground">
                {persona.displayName}
              </span>
            </span>
            <ChevronRight
              aria-hidden="true"
              className="mt-3 h-4 w-4 shrink-0 text-muted-foreground transition-transform group-hover:translate-x-0.5"
            />
          </button>
          <AgentTemplateImpactPreview
            agents={affectedAgents}
            className="px-4 pb-4 pl-[4.5rem]"
            surface="plain"
          />
        </div>
      </ChooserDialogContent>
    </Dialog>
  );
}
