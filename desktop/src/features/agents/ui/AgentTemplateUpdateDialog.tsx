import { Check, Minus, RefreshCw, RotateCcw, XCircle } from "lucide-react";
import * as React from "react";

import type {
  AgentTemplateUpdateProgressStage,
  AgentTemplateUpdatePreview,
  ApplyAgentTemplateUpdateResponse,
} from "@/shared/api/tauriAgentTemplateUpdates";
import { agentTemplateUpdateProgressLabel } from "@/shared/api/tauriAgentTemplateUpdates";
import type { AgentTemplateVersionRef } from "@/shared/api/types";
import { shortAgentTemplateVersionToken } from "../lib/agentTemplateUpdatePreview";
import { Button } from "@/shared/ui/button";
import { Checkbox } from "@/shared/ui/checkbox";
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
import {
  AgentTemplateInstructionChangesSummary,
  commonAgentInstructionChange,
} from "./AgentTemplateInstructionUpdateFields";
import {
  AgentTemplateOverridesPreservedSummary,
  AgentTemplateVersionChangesSummary,
  commonAgentVersionChanges,
} from "./AgentTemplateVersionUpdateFields";

type AgentTemplateUpdateDialogProps = {
  error: string | null;
  isPending: boolean;
  onApply: (
    selectedPubkeys: string[],
    connectionBindingsByPubkey: Record<string, Record<string, string>>,
  ) => void;
  onOpenAgent?: (pubkey: string) => void;
  onOpenChange: (open: boolean) => void;
  open: boolean;
  preview: AgentTemplateUpdatePreview | null;
  progressByPubkey: Record<string, AgentTemplateUpdateProgressStage>;
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
  onOpenAgent,
  onOpenChange,
  open,
  preview,
  progressByPubkey,
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
  const commonInstructionChange = commonAgentInstructionChange(changedAgents);
  const commonToolChanges = commonAgentToolChanges(changedAgents);
  const commonSkillChanges = commonAgentSkillChanges(changedAgents);
  const commonVersionChanges = commonAgentVersionChanges(changedAgents);
  const bindingsReady = changedAgents
    .filter((agent) => selected.has(agent.pubkey))
    .every(
      (agent) =>
        preview.targetToolRequirements.length === 0 ||
        bindingsValidByPubkey[agent.pubkey] === true,
    );
  const activeProgressStage = progressStage ?? "preparing_update";
  const progressComplete = activeProgressStage === "updated";
  const displayedAgents = isPending
    ? preview.agents.filter((agent) => selected.has(agent.pubkey))
    : preview.agents;
  const hasFailed = !isPending && !isComplete && error !== null;
  const dialogTitle = isComplete
    ? rollbackFailed
      ? `${preview.personaName} needs attention`
      : result.rolledBack
        ? `${preview.personaName} update rolled back`
        : `${preview.personaName} updated`
    : isPending
      ? `Updating ${preview.personaName}`
      : hasFailed
        ? `${preview.personaName} update failed`
        : `Update ${preview.personaName}`;

  function setAgentSelected(pubkey: string, checked: boolean) {
    setSelected((current) => {
      const next = new Set(current);
      if (checked) next.add(pubkey);
      else next.delete(pubkey);
      return next;
    });
  }

  return (
    <Dialog onOpenChange={onOpenChange} open={open}>
      <DialogContent
        className="flex max-h-[85vh] max-w-2xl flex-col overflow-hidden"
        data-testid="template-publish-review"
      >
        <DialogHeader className="shrink-0">
          <DialogTitle className="break-words">{dialogTitle}</DialogTitle>
          <DialogDescription>
            {isComplete
              ? rollbackFailed
                ? "Buzz could not restore every agent automatically. Review the agents below."
                : result.rolledBack
                  ? "The previous version was restored. The template edit is still saved."
                  : `${result.agents.length} ${
                      result.agents.length === 1 ? "agent was" : "agents were"
                    } updated.`
              : isPending
                ? `${selectedCount} ${
                    selectedCount === 1 ? "agent" : "agents"
                  } using ${preview.personaName} ${
                    selectedCount === 1 ? "is" : "are"
                  } being updated.`
                : hasFailed
                  ? `Buzz could not update the selected agents using ${preview.personaName}.`
                  : `${preview.personaName} is used by ${preview.agents.length} ${
                      preview.agents.length === 1 ? "agent" : "agents"
                    }. Choose who should get this version now.`}
          </DialogDescription>
        </DialogHeader>

        <div className="min-h-0 flex-1 space-y-4 overflow-y-auto pr-1">
          {error ? (
            <div
              className="rounded-xl border border-destructive/30 bg-destructive/5 p-4"
              role="alert"
            >
              <p className="text-sm font-medium text-destructive">
                The agents were not updated.
              </p>
              <p className="mt-1 text-xs text-destructive">{error}</p>
            </div>
          ) : null}

          {isPending ? (
            <div
              aria-live="polite"
              className="rounded-xl border border-border bg-muted/30 p-5"
              data-testid="template-rollout-progress"
            >
              <div className="flex items-center gap-3">
                {progressComplete ? (
                  <Check
                    aria-hidden="true"
                    className="size-5 text-status-added"
                  />
                ) : (
                  <RefreshCw
                    aria-hidden="true"
                    className="size-5 text-primary motion-safe:animate-spin"
                  />
                )}
                <p className="text-sm font-medium">
                  {agentTemplateUpdateProgressLabel(activeProgressStage)}
                </p>
              </div>
              <p className="mt-3 text-xs text-muted-foreground">
                This update will continue in the background if you close this
                window.
              </p>
            </div>
          ) : null}

          <div
            className="overflow-hidden rounded-xl border border-border"
            data-testid="template-update-agents"
          >
            <div className="border-b border-border bg-muted/20 px-4 py-3">
              <p className="text-sm font-medium">
                {isComplete
                  ? "Agent results"
                  : isPending
                    ? `Updating ${selectedCount} ${
                        selectedCount === 1 ? "agent" : "agents"
                      }`
                    : "Affected agents"}
              </p>
              {!isComplete && !isPending ? (
                <p className="mt-0.5 text-xs text-muted-foreground">
                  {selectedCount} of {changedAgents.length} selected
                </p>
              ) : null}
            </div>
            {displayedAgents.map((agent) => {
              const agentResult = result?.agents.find(
                (candidate) => candidate.pubkey === agent.pubkey,
              );
              const checked = selected.has(agent.pubkey);
              const agentProgressStage = progressByPubkey[agent.pubkey];
              const isCurrent =
                agent.currentVersion === preview.targetVersionToken;
              const checkboxId = `template-update-checkbox-${agent.pubkey}`;
              const Row = "div";
              const headerContent = (
                <>
                  <span
                    className="truncate text-sm font-medium"
                    title={agent.name}
                  >
                    {agent.name}
                  </span>
                  {!isComplete ? (
                    <span className="inline-flex items-center gap-1.5 text-xs text-muted-foreground">
                      <span
                        aria-hidden="true"
                        className={cn(
                          "size-1.5 rounded-full",
                          agent.runningRelays.length > 0
                            ? "bg-status-added"
                            : "bg-muted-foreground/50",
                        )}
                      />
                      {agent.runningRelays.length > 0 ? "Running" : "Stopped"}
                    </span>
                  ) : null}
                </>
              );
              return (
                <Row
                  className={cn(
                    "flex items-start gap-3 border-b border-border px-4 py-3 last:border-b-0",
                    (!agent.eligible || isCurrent) && "bg-muted/10",
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
                        className="size-4 text-status-added"
                      />
                    ) : agentResult.outcome === "rolled_back" ? (
                      <RotateCcw
                        aria-label="Rolled back"
                        className="size-4 text-status-modified"
                      />
                    ) : (
                      <XCircle
                        aria-label="Needs attention"
                        className="size-4 text-destructive"
                      />
                    )
                  ) : isPending ? (
                    agentProgressStage === "updated" ? (
                      <Check
                        aria-label="Updated"
                        className="size-4 text-status-added"
                      />
                    ) : agentProgressStage ? (
                      <RefreshCw
                        aria-label={agentTemplateUpdateProgressLabel(
                          agentProgressStage,
                        )}
                        className="size-4 text-primary motion-safe:animate-spin"
                      />
                    ) : (
                      <span
                        aria-label="Queued"
                        className="mt-1 size-2 rounded-full bg-muted-foreground/40"
                        role="img"
                      />
                    )
                  ) : (
                    <Checkbox
                      aria-label={`Update ${agent.name}`}
                      checked={checked}
                      disabled={isPending || !agent.eligible || isCurrent}
                      id={checkboxId}
                      onCheckedChange={(nextChecked) =>
                        setAgentSelected(agent.pubkey, nextChecked === true)
                      }
                    />
                  )}
                  <div className="min-w-0 flex-1">
                    {isComplete ? (
                      <div className="flex flex-wrap items-center gap-x-3 gap-y-1">
                        {headerContent}
                      </div>
                    ) : (
                      <label
                        className={cn(
                          "flex flex-wrap items-center gap-x-3 gap-y-1",
                          agent.eligible &&
                            !isCurrent &&
                            !isPending &&
                            "cursor-pointer rounded-sm hover:text-foreground/80",
                        )}
                        htmlFor={checkboxId}
                      >
                        {headerContent}
                      </label>
                    )}
                    <p className="mt-1 text-xs text-muted-foreground">
                      {agentResult
                        ? agentResult.outcome === "updated"
                          ? "Updated and ready"
                          : agentResult.outcome === "updated_stopped"
                            ? "Updated · starts on this version next time"
                            : agentResult.outcome === "rolled_back"
                              ? "Previous version restored"
                              : "Previous version was not restored"
                        : isPending
                          ? agentProgressStage
                            ? agentTemplateUpdateProgressLabel(
                                agentProgressStage,
                              )
                            : "Queued"
                          : isComplete
                            ? "Not updated"
                            : isCurrent
                              ? `Already using ${shortPublishedVersion(preview.targetVersion)}`
                              : `Version ${shortAgentTemplateVersionToken(agent.currentVersion)} → ${shortPublishedVersion(
                                  preview.targetVersion,
                                )}`}
                    </p>
                    {agent.blockedReason ? (
                      <p
                        className={cn("mt-1 text-xs", "text-muted-foreground")}
                      >
                        {agent.blockedReason}
                      </p>
                    ) : null}
                    {agentResult?.outcome === "rollback_failed" ? (
                      <div className="mt-3 space-y-2">
                        <p className="text-xs text-destructive">
                          Buzz could not restore the previous version.
                        </p>
                        <div className="flex flex-wrap items-center gap-2">
                          {onOpenAgent ? (
                            <Button
                              onClick={() => onOpenAgent(agent.pubkey)}
                              size="xs"
                              type="button"
                              variant="outline"
                            >
                              Open agent
                            </Button>
                          ) : null}
                          {agentResult.error ? (
                            <details className="text-xs text-muted-foreground">
                              <summary className="cursor-pointer">
                                Technical details
                              </summary>
                              <p className="mt-1 break-words">
                                {agentResult.error}
                              </p>
                            </details>
                          ) : null}
                        </div>
                      </div>
                    ) : null}
                    {!isComplete &&
                    !isPending &&
                    !commonInstructionChange &&
                    !isCurrent &&
                    agent.instructionChange ? (
                      <div className="mt-3">
                        <AgentTemplateInstructionChangesSummary
                          change={agent.instructionChange}
                        />
                      </div>
                    ) : null}
                    {!isComplete &&
                    !isPending &&
                    !commonVersionChanges &&
                    !isCurrent ? (
                      <div className="mt-3">
                        <AgentTemplateVersionChangesSummary
                          changes={agent.versionChanges}
                        />
                      </div>
                    ) : null}
                    {!isComplete &&
                    !isPending &&
                    !commonToolChanges &&
                    !isCurrent ? (
                      <div className="mt-3">
                        <AgentTemplateToolChangesSummary
                          changes={agent.toolChanges}
                        />
                      </div>
                    ) : null}
                    {!isComplete &&
                    !isPending &&
                    !commonSkillChanges &&
                    !isCurrent ? (
                      <div className="mt-3">
                        <AgentTemplateSkillChangesSummary
                          changes={agent.skillChanges}
                        />
                      </div>
                    ) : null}
                    {!isComplete && !isPending && !isCurrent ? (
                      <AgentTemplateOverridesPreservedSummary
                        overrides={agent.overridesPreserved}
                      />
                    ) : null}
                    {!isComplete &&
                    !isPending &&
                    checked &&
                    agent.eligible &&
                    !isCurrent ? (
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
                    !isPending &&
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
          {!isComplete && selectedCount > 0 && !bindingsReady ? (
            <p className="text-sm text-destructive" role="alert">
              Choose a ready connection for each required tool before updating.
            </p>
          ) : null}

          {!isComplete && !isPending && commonInstructionChange ? (
            <AgentTemplateInstructionChangesSummary
              change={commonInstructionChange}
            />
          ) : null}
          {!isComplete && !isPending && commonVersionChanges ? (
            <AgentTemplateVersionChangesSummary
              changes={commonVersionChanges}
            />
          ) : null}
          {!isComplete && !isPending && commonToolChanges ? (
            <AgentTemplateToolChangesSummary changes={commonToolChanges} />
          ) : null}
          {!isComplete && !isPending && commonSkillChanges ? (
            <AgentTemplateSkillChangesSummary changes={commonSkillChanges} />
          ) : null}

          {!isComplete &&
          !isPending &&
          preview.agents.some(
            (agent) =>
              selected.has(agent.pubkey) && agent.runningRelays.length > 0,
          ) ? (
            <p className="rounded-lg border border-border bg-muted/30 px-3 py-2 text-xs text-muted-foreground">
              Buzz finishes current work before switching versions. If the
              update fails, it restores the previous version.
            </p>
          ) : null}
        </div>

        <DialogFooter className="shrink-0">
          {isComplete ? (
            result.rolledBack && !rollbackFailed ? (
              <>
                <Button
                  onClick={() => onOpenChange(false)}
                  type="button"
                  variant="outline"
                >
                  Done
                </Button>
                <Button
                  disabled={selectedCount === 0 || !bindingsReady}
                  onClick={() => onApply([...selected], bindingsByPubkey)}
                  type="button"
                >
                  Try update again
                </Button>
              </>
            ) : (
              <Button onClick={() => onOpenChange(false)} type="button">
                Done
              </Button>
            )
          ) : !hasEligibleChanges ? (
            <Button onClick={() => onOpenChange(false)} type="button">
              Done
            </Button>
          ) : (
            <>
              <Button
                onClick={() => onOpenChange(false)}
                type="button"
                variant="outline"
              >
                {isPending ? "Continue working" : "Not now"}
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
                {isPending
                  ? "Updating…"
                  : `Update ${selectedCount} ${
                      selectedCount === 1 ? "agent" : "agents"
                    }`}
              </Button>
            </>
          )}
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
