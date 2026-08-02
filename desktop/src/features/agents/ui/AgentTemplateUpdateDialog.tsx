import * as React from "react";
import { Check, Minus, RefreshCw, RotateCcw, XCircle } from "lucide-react";

import type {
  AgentTemplateUpdatePreview,
  ApplyAgentTemplateUpdateResponse,
} from "@/shared/api/tauriAgentTemplateUpdates";
import { Badge } from "@/shared/ui/badge";
import { Button } from "@/shared/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/shared/ui/dialog";
import { cn } from "@/shared/lib/cn";

type AgentTemplateUpdateDialogProps = {
  error: string | null;
  isPending: boolean;
  onApply: (selectedPubkeys: string[]) => void;
  onOpenChange: (open: boolean) => void;
  open: boolean;
  preview: AgentTemplateUpdatePreview | null;
  result: ApplyAgentTemplateUpdateResponse | null;
};

function shortVersion(version: string | null): string {
  return version ? version.slice(0, 7) : "unversioned";
}

export function AgentTemplateUpdateDialog({
  error,
  isPending,
  onApply,
  onOpenChange,
  open,
  preview,
  result,
}: AgentTemplateUpdateDialogProps) {
  const [selected, setSelected] = React.useState<Set<string>>(new Set());

  React.useEffect(() => {
    if (!open || !preview) return;
    setSelected(
      new Set(
        preview.agents
          .filter(
            (agent) =>
              agent.eligible && agent.currentVersion !== preview.targetVersion,
          )
          .map((agent) => agent.pubkey),
      ),
    );
  }, [open, preview]);

  if (!preview) return null;

  const selectedCount = selected.size;
  const changedAgents = preview.agents.filter(
    (agent) => agent.currentVersion !== preview.targetVersion,
  );
  const hasEligibleChanges = changedAgents.some((agent) => agent.eligible);
  const isComplete = result !== null;
  const rollbackFailed =
    result?.agents.some((agent) => agent.outcome === "rollback_failed") ??
    false;

  return (
    <Dialog onOpenChange={onOpenChange} open={open}>
      <DialogContent
        className="max-h-[85vh] max-w-2xl overflow-y-auto"
        data-testid="template-publish-review"
      >
        <DialogHeader>
          <DialogTitle>
            {isComplete
              ? rollbackFailed
                ? "Some agents need attention"
                : result.rolledBack
                  ? "Update rolled back"
                  : "Agents updated"
              : `Update agents using ${preview.personaName}?`}
          </DialogTitle>
          <DialogDescription>
            {isComplete
              ? rollbackFailed
                ? "Buzz could not restore every agent automatically. Review the agents below."
                : result.rolledBack
                  ? "The previous version was restored. The template edit is still saved."
                  : `${result.agents.length} ${
                      result.agents.length === 1 ? "agent was" : "agents were"
                    } updated.`
              : `${preview.personaName} is used by ${preview.agents.length} ${
                  preview.agents.length === 1 ? "agent" : "agents"
                }. Choose which agents should use the new version now.`}
          </DialogDescription>
        </DialogHeader>

        {isPending ? (
          <div
            aria-live="polite"
            className="rounded-xl border border-border bg-muted/30 p-5"
            data-testid="template-rollout-progress"
          >
            <div className="flex items-center gap-3">
              <RefreshCw
                aria-hidden="true"
                className="size-5 animate-spin text-primary"
              />
              <div>
                <p className="text-sm font-medium">Updating agents</p>
                <p className="mt-1 text-xs text-muted-foreground">
                  Starting the new configuration and checking each connection.
                  If a check fails, Buzz attempts to restore the previous
                  version.
                </p>
              </div>
            </div>
          </div>
        ) : (
          <div className="space-y-2">
            {preview.agents.map((agent) => {
              const agentResult = result?.agents.find(
                (candidate) => candidate.pubkey === agent.pubkey,
              );
              const checked = selected.has(agent.pubkey);
              const isCurrent = agent.currentVersion === preview.targetVersion;
              const Row = isComplete ? "div" : "label";
              return (
                <Row
                  className={cn(
                    "flex items-center gap-3 rounded-xl border border-border px-4 py-3",
                    !isComplete &&
                      agent.eligible &&
                      !isCurrent &&
                      "cursor-pointer hover:bg-muted/30",
                    (!agent.eligible || isCurrent) && "opacity-70",
                  )}
                  data-testid={`template-update-agent-${agent.pubkey}`}
                  key={agent.pubkey}
                >
                  {isComplete ? (
                    !agentResult ? (
                      <Minus
                        aria-label="Not updated"
                        className="size-4 text-muted-foreground"
                      />
                    ) : agentResult.outcome === "updated" ||
                      agentResult.outcome === "updated_stopped" ? (
                      <Check
                        aria-label="Updated"
                        className="size-4 text-emerald-500"
                      />
                    ) : agentResult.outcome === "rolled_back" ? (
                      <RotateCcw
                        aria-label="Rolled back"
                        className="size-4 text-amber-500"
                      />
                    ) : (
                      <XCircle
                        aria-label="Needs attention"
                        className="size-4 text-destructive"
                      />
                    )
                  ) : (
                    <input
                      aria-label={`Update ${agent.name}`}
                      checked={checked}
                      disabled={!agent.eligible || isCurrent}
                      onChange={(event) => {
                        setSelected((current) => {
                          const next = new Set(current);
                          if (event.target.checked) next.add(agent.pubkey);
                          else next.delete(agent.pubkey);
                          return next;
                        });
                      }}
                      type="checkbox"
                    />
                  )}
                  <div className="min-w-0 flex-1">
                    <div className="flex flex-wrap items-center gap-2">
                      <span className="truncate text-sm font-medium">
                        {agent.name}
                      </span>
                      {agent.runningRelays.length > 0 ? (
                        <Badge variant="secondary">Running</Badge>
                      ) : (
                        <Badge variant="outline">Stopped</Badge>
                      )}
                    </div>
                    <p className="mt-1 text-xs text-muted-foreground">
                      {agentResult
                        ? agentResult.outcome === "updated"
                          ? "Updated and ready"
                          : agentResult.outcome === "updated_stopped"
                            ? "Updated · starts on this version next time"
                            : agentResult.outcome === "rolled_back"
                              ? "Previous version restored"
                              : (agentResult.error ??
                                "Rollback needs attention")
                        : isComplete
                          ? "Not updated"
                          : isCurrent
                            ? `Already using ${shortVersion(preview.targetVersion)}`
                            : `${shortVersion(agent.currentVersion)} → ${shortVersion(
                                preview.targetVersion,
                              )}`}
                    </p>
                    {agent.blockedReason ? (
                      <p
                        className={cn(
                          "mt-1 text-xs",
                          isComplete
                            ? "text-muted-foreground"
                            : "text-destructive",
                        )}
                      >
                        {agent.blockedReason}
                      </p>
                    ) : null}
                  </div>
                </Row>
              );
            })}
          </div>
        )}

        {error ? (
          <p className="text-sm text-destructive" role="alert">
            {error}
          </p>
        ) : null}

        {!isComplete &&
        !isPending &&
        preview.agents.some(
          (agent) =>
            selected.has(agent.pubkey) && agent.runningRelays.length > 0,
        ) ? (
          <p className="rounded-lg bg-amber-500/10 px-3 py-2 text-xs text-muted-foreground">
            Running agents restart briefly. Update after any important active
            work has finished.
          </p>
        ) : null}

        <DialogFooter>
          {isComplete ? (
            <Button onClick={() => onOpenChange(false)} type="button">
              Done
            </Button>
          ) : !hasEligibleChanges ? (
            <Button onClick={() => onOpenChange(false)} type="button">
              Done
            </Button>
          ) : (
            <>
              <Button
                disabled={isPending}
                onClick={() => onOpenChange(false)}
                type="button"
                variant="outline"
              >
                Update later
              </Button>
              <Button
                data-testid="template-publish-and-update"
                disabled={
                  isPending || selectedCount === 0 || changedAgents.length === 0
                }
                onClick={() => onApply([...selected])}
                type="button"
              >
                Update {selectedCount}{" "}
                {selectedCount === 1 ? "agent" : "agents"}
              </Button>
            </>
          )}
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
