import * as React from "react";
import { Check, Minus, RefreshCw, RotateCcw, XCircle } from "lucide-react";

import type {
  AgentTemplateUpdateProgressStage,
  AgentTemplateUpdatePreview,
  ApplyAgentTemplateUpdateResponse,
} from "@/shared/api/tauriAgentTemplateUpdates";
import { agentTemplateUpdateProgressLabel } from "@/shared/api/tauriAgentTemplateUpdates";
import type { AgentTemplateVersionRef } from "@/shared/api/types";
import { shortAgentTemplateVersionToken } from "../lib/agentTemplateUpdatePreview";
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
import {
  AgentTemplateToolBindings,
  AgentTemplateToolChangesSummary,
  bindingsForToolRequirements,
  commonAgentToolChanges,
} from "./AgentTemplateToolUpdateFields";
import {
  AgentTemplateSkillChangesSummary,
  commonAgentSkillChanges,
} from "./AgentTemplateSkillUpdateFields";

type AgentTemplateUpdateDialogProps = {
  error: string | null;
  isPending: boolean;
  onApply: (
    selectedPubkeys: string[],
    connectionBindingsByPubkey: Record<string, Record<string, string>>,
  ) => void;
  onOpenChange: (open: boolean) => void;
  open: boolean;
  preview: AgentTemplateUpdatePreview | null;
  progressStage: AgentTemplateUpdateProgressStage | null;
  result: ApplyAgentTemplateUpdateResponse | null;
};

function shortPublishedVersion(version: AgentTemplateVersionRef): string {
  return version.commitOid.slice(0, 7);
}

export function AgentTemplateUpdateDialog({
  error,
  isPending,
  onApply,
  onOpenChange,
  open,
  preview,
  progressStage,
  result,
}: AgentTemplateUpdateDialogProps) {
  const [selected, setSelected] = React.useState<Set<string>>(new Set());
  const [bindingsByPubkey, setBindingsByPubkey] = React.useState<
    Record<string, Record<string, string>>
  >({});
  const [bindingsValidByPubkey, setBindingsValidByPubkey] = React.useState<
    Record<string, boolean>
  >({});

  React.useEffect(() => {
    if (!open || !preview) return;
    setSelected(
      new Set(
        preview.agents
          .filter(
            (agent) =>
              agent.eligible &&
              agent.currentVersion !== preview.targetVersionToken,
          )
          .map((agent) => agent.pubkey),
      ),
    );
    setBindingsByPubkey(
      Object.fromEntries(
        preview.agents.map((agent) => [
          agent.pubkey,
          bindingsForToolRequirements(
            agent.connectionBindings,
            preview.targetToolRequirements,
          ),
        ]),
      ),
    );
    setBindingsValidByPubkey({});
  }, [open, preview]);
  const handleBindingsValidityChange = React.useCallback(
    (pubkey: string, valid: boolean) => {
      setBindingsValidByPubkey((current) =>
        current[pubkey] === valid ? current : { ...current, [pubkey]: valid },
      );
    },
    [],
  );

  if (!preview) return null;

  const selectedCount = selected.size;
  const changedAgents = preview.agents.filter(
    (agent) => agent.currentVersion !== preview.targetVersionToken,
  );
  const hasEligibleChanges = changedAgents.some((agent) => agent.eligible);
  const isComplete = result !== null;
  const rollbackFailed =
    result?.agents.some((agent) => agent.outcome === "rollback_failed") ??
    false;
  const commonToolChanges = commonAgentToolChanges(changedAgents);
  const commonSkillChanges = commonAgentSkillChanges(changedAgents);
  const bindingsReady = changedAgents
    .filter((agent) => selected.has(agent.pubkey))
    .every(
      (agent) =>
        preview.targetToolRequirements.length === 0 ||
        bindingsValidByPubkey[agent.pubkey] === true,
    );
  const activeProgressStage = progressStage ?? "preparing_update";
  const progressComplete = activeProgressStage === "updated";

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

        {!isComplete && commonToolChanges ? (
          <AgentTemplateToolChangesSummary changes={commonToolChanges} />
        ) : null}
        {!isComplete && commonSkillChanges ? (
          <AgentTemplateSkillChangesSummary changes={commonSkillChanges} />
        ) : null}

        {isPending ? (
          <div
            aria-live="polite"
            className="rounded-xl border border-border bg-muted/30 p-5"
            data-testid="template-rollout-progress"
          >
            <div className="flex items-center gap-3">
              {progressComplete ? (
                <Check aria-hidden="true" className="size-5 text-emerald-500" />
              ) : (
                <RefreshCw
                  aria-hidden="true"
                  className="size-5 animate-spin text-primary"
                />
              )}
              <p className="text-sm font-medium">
                {agentTemplateUpdateProgressLabel(activeProgressStage)}
              </p>
            </div>
          </div>
        ) : (
          <div className="space-y-2">
            {preview.agents.map((agent) => {
              const agentResult = result?.agents.find(
                (candidate) => candidate.pubkey === agent.pubkey,
              );
              const checked = selected.has(agent.pubkey);
              const isCurrent =
                agent.currentVersion === preview.targetVersionToken;
              const Row = "div";
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
                            ? `Already using ${shortPublishedVersion(preview.targetVersion)}`
                            : `${shortAgentTemplateVersionToken(agent.currentVersion)} → ${shortPublishedVersion(
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
                    {!isComplete && !commonToolChanges && !isCurrent ? (
                      <div className="mt-3">
                        <AgentTemplateToolChangesSummary
                          changes={agent.toolChanges}
                        />
                      </div>
                    ) : null}
                    {!isComplete && !commonSkillChanges && !isCurrent ? (
                      <div className="mt-3">
                        <AgentTemplateSkillChangesSummary
                          changes={agent.skillChanges}
                        />
                      </div>
                    ) : null}
                    {!isComplete && checked && agent.eligible && !isCurrent ? (
                      <AgentTemplateToolBindings
                        agent={agent}
                        bindings={bindingsByPubkey[agent.pubkey] ?? {}}
                        disabled={isPending}
                        onBindingsChange={(bindings) =>
                          setBindingsByPubkey((current) => ({
                            ...current,
                            [agent.pubkey]: bindings,
                          }))
                        }
                        onValidityChange={(valid) =>
                          handleBindingsValidityChange(agent.pubkey, valid)
                        }
                        requirements={preview.targetToolRequirements}
                      />
                    ) : null}
                    {!isComplete &&
                    bindingsValidByPubkey[agent.pubkey] !== true &&
                    agent.toolBindingIssues.length > 0 ? (
                      <ul className="mt-2 list-disc space-y-1 pl-4 text-xs text-destructive">
                        {agent.toolBindingIssues.map((issue) => (
                          <li key={issue}>{issue}</li>
                        ))}
                      </ul>
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
        {!isComplete && selectedCount > 0 && !bindingsReady ? (
          <p className="text-sm text-destructive" role="alert">
            Choose a ready connection for each required tool before updating.
          </p>
        ) : null}

        {!isComplete &&
        !isPending &&
        preview.agents.some(
          (agent) =>
            selected.has(agent.pubkey) && agent.runningRelays.length > 0,
        ) ? (
          <p className="rounded-lg border border-border bg-muted/30 px-3 py-2 text-xs text-muted-foreground">
            Safe to update now. Buzz stops new work and lets the current task
            finish. If it cannot, the new version recovers it after the update.
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
                  isPending ||
                  selectedCount === 0 ||
                  changedAgents.length === 0 ||
                  !bindingsReady
                }
                onClick={() => onApply([...selected], bindingsByPubkey)}
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
