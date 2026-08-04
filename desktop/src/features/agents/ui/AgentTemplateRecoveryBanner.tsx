import { useMutation, useQuery } from "@tanstack/react-query";
import { relaunch } from "@tauri-apps/plugin-process";
import { AlertTriangle, RotateCcw } from "lucide-react";
import * as React from "react";

import {
  listAgentTemplateUpdateRecoveries,
  quarantineInvalidAgentTemplateUpdates,
  restoreInterruptedAgentTemplateUpdate,
  type AgentTemplateUpdateRecoveryStatus,
} from "@/shared/api/tauriAgentTemplateUpdates";
import { Button } from "@/shared/ui/button";
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from "@/shared/ui/alert-dialog";

type AgentTemplateRecoveryBannerProps = {
  onRecovered: () => void;
  templateNamesById: ReadonlyMap<string, string>;
};

const RECOVERY_QUERY_KEY = ["agent-template-update-recoveries"] as const;

function agentList(status: AgentTemplateUpdateRecoveryStatus): string {
  return status.agents.map((agent) => agent.name).join(", ");
}

function templateName(
  status: AgentTemplateUpdateRecoveryStatus,
  namesById: ReadonlyMap<string, string>,
): string {
  return status.templateId
    ? (namesById.get(status.templateId) ?? "Agent template")
    : "Unverified agent update";
}

export function AgentTemplateRecoveryBanner({
  onRecovered,
  templateNamesById,
}: AgentTemplateRecoveryBannerProps) {
  const [selected, setSelected] =
    React.useState<AgentTemplateUpdateRecoveryStatus | null>(null);
  const [invalidReviewOpen, setInvalidReviewOpen] = React.useState(false);
  const [relaunchRequired, setRelaunchRequired] = React.useState(false);
  const recoveries = useQuery({
    queryKey: RECOVERY_QUERY_KEY,
    queryFn: listAgentTemplateUpdateRecoveries,
    refetchOnWindowFocus: true,
    staleTime: 0,
  });
  const restore = useMutation({
    mutationFn: restoreInterruptedAgentTemplateUpdate,
    onSuccess: async () => {
      setSelected(null);
      await recoveries.refetch();
      onRecovered();
    },
  });
  const quarantine = useMutation({
    mutationFn: quarantineInvalidAgentTemplateUpdates,
    onSuccess: async (result) => {
      setInvalidReviewOpen(false);
      if (result.relaunchRequired) {
        setRelaunchRequired(true);
      }
      await recoveries.refetch();
    },
  });

  const allAttention = (recoveries.data ?? []).filter(
    (status) => status.requiresAttention,
  );
  const invalidAttention = allAttention.filter(
    (status) => !status.transactionId,
  );
  const attention = [
    ...allAttention.filter((status) => status.transactionId),
    ...invalidAttention.slice(0, 1),
  ];
  const queryError =
    recoveries.error instanceof Error ? recoveries.error.message : null;
  const restoreError =
    restore.error instanceof Error ? restore.error.message : null;
  const quarantineError =
    quarantine.error instanceof Error ? quarantine.error.message : null;

  if (relaunchRequired) {
    return (
      <section
        className="rounded-lg border border-warning/40 bg-warning-bg px-4 py-3"
        data-testid="agent-update-recovery-relaunch"
        role="alert"
      >
        <div className="flex items-start gap-3">
          <AlertTriangle
            aria-hidden="true"
            className="mt-0.5 size-4 shrink-0 text-warning"
          />
          <div className="min-w-0 flex-1">
            <h2 className="text-sm font-semibold text-foreground">
              Restart Buzz to finish recovery
            </h2>
            <p className="mt-0.5 text-xs leading-5 text-muted-foreground">
              The update record is preserved. Agent starts and edits remain
              paused until Buzz restarts and checks for affected agent
              processes.
            </p>
          </div>
          <Button
            className="shrink-0"
            data-testid="agent-update-recovery-relaunch-button"
            onClick={() => {
              void relaunch();
            }}
            size="sm"
            type="button"
          >
            Restart Buzz
          </Button>
        </div>
      </section>
    );
  }

  if (!queryError && attention.length === 0) return null;

  return (
    <>
      <section
        className="rounded-lg border border-warning/40 bg-warning-bg px-4 py-3"
        data-testid="agent-update-recovery-banner"
        role="alert"
      >
        <div className="flex items-start gap-3">
          <AlertTriangle
            aria-hidden="true"
            className="mt-0.5 size-4 shrink-0 text-warning"
          />
          <div className="min-w-0 flex-1 space-y-3">
            <div>
              <h2 className="text-sm font-semibold text-foreground">
                Agent updates are paused
              </h2>
              <p className="mt-0.5 text-xs leading-5 text-muted-foreground">
                Buzz found an update that needs attention. Agent starts and
                edits stay paused until recovery finishes.
              </p>
            </div>

            {queryError ? (
              <div className="flex flex-wrap items-center justify-between gap-2">
                <p className="text-xs text-destructive">
                  Buzz could not check the recovery state. {queryError}
                </p>
                <Button
                  disabled={recoveries.isFetching}
                  onClick={() => {
                    void recoveries.refetch();
                  }}
                  size="sm"
                  type="button"
                  variant="outline"
                >
                  Check again
                </Button>
              </div>
            ) : null}

            {attention.map((status, index) => {
              const key = status.transactionId ?? `invalid-${index}`;
              const names = agentList(status);
              const name = status.transactionId
                ? templateName(status, templateNamesById)
                : invalidAttention.length === 1
                  ? "Unverified agent update"
                  : `${invalidAttention.length} unverified agent updates`;
              return (
                <div
                  className="flex flex-wrap items-center justify-between gap-3 border-t border-warning/20 pt-3"
                  data-testid="agent-update-recovery-item"
                  key={key}
                >
                  <div className="min-w-0">
                    <p className="truncate text-sm font-medium text-foreground">
                      {name}
                    </p>
                    {names ? (
                      <p className="truncate text-xs text-muted-foreground">
                        {names}
                      </p>
                    ) : null}
                    <p className="mt-1 text-xs leading-5 text-muted-foreground">
                      {status.transactionId
                        ? status.detail
                        : "Buzz cannot verify which agents were affected."}
                    </p>
                  </div>
                  {status.transactionId ? (
                    <Button
                      className="shrink-0"
                      data-testid="agent-update-recovery-open"
                      onClick={() => {
                        restore.reset();
                        setSelected(status);
                      }}
                      size="sm"
                      type="button"
                      variant="outline"
                    >
                      <RotateCcw aria-hidden="true" />
                      Review recovery
                    </Button>
                  ) : (
                    <Button
                      className="shrink-0"
                      data-testid="agent-update-invalid-recovery-open"
                      onClick={() => {
                        quarantine.reset();
                        setInvalidReviewOpen(true);
                      }}
                      size="sm"
                      type="button"
                      variant="outline"
                    >
                      Continue recovery
                    </Button>
                  )}
                </div>
              );
            })}
          </div>
        </div>
      </section>

      <AlertDialog
        onOpenChange={(open) => {
          if (!open && !restore.isPending) {
            setSelected(null);
            restore.reset();
          }
        }}
        open={selected !== null}
      >
        <AlertDialogContent
          className="max-w-lg"
          data-testid="agent-update-recovery-confirmation"
        >
          <AlertDialogHeader>
            <AlertDialogTitle>
              Restore the previous{" "}
              {selected
                ? templateName(selected, templateNamesById)
                : "this template"}{" "}
              version?
            </AlertDialogTitle>
            <AlertDialogDescription>
              Buzz will stop the affected agents, then restore the version they
              used before the interrupted update. Work accepted during that
              update may be incomplete.
            </AlertDialogDescription>
          </AlertDialogHeader>

          {selected?.agents.length ? (
            <div className="rounded-lg border border-border bg-muted/30 px-3 py-2">
              <p className="text-xs font-medium text-foreground">
                {agentList(selected)}
              </p>
            </div>
          ) : null}

          {restoreError ? (
            <p className="text-sm text-destructive" role="alert">
              Previous version wasn’t restored. {restoreError} Fix the problem
              and try again.
            </p>
          ) : null}

          <AlertDialogFooter>
            <AlertDialogCancel disabled={restore.isPending}>
              Cancel
            </AlertDialogCancel>
            <AlertDialogAction
              disabled={restore.isPending || !selected?.transactionId}
              onClick={(event) => {
                event.preventDefault();
                if (selected?.transactionId) {
                  restore.mutate(selected.transactionId);
                }
              }}
            >
              {restore.isPending ? "Restoring..." : "Stop agents and restore"}
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>

      <AlertDialog
        onOpenChange={(open) => {
          if (!open && !quarantine.isPending) {
            setInvalidReviewOpen(false);
            quarantine.reset();
          }
        }}
        open={invalidReviewOpen}
      >
        <AlertDialogContent
          className="max-w-lg"
          data-testid="agent-update-invalid-recovery-confirmation"
        >
          <AlertDialogHeader>
            <AlertDialogTitle>Continue agent recovery?</AlertDialogTitle>
            <AlertDialogDescription>
              Buzz cannot verify which agents were affected. It will preserve
              the update record, then stop using it for recovery. Restart Buzz
              next so it can check for affected agent processes before agents
              can run again.
            </AlertDialogDescription>
          </AlertDialogHeader>

          {quarantineError ? (
            <p className="text-sm text-destructive" role="alert">
              Recovery couldn’t continue. {quarantineError}
            </p>
          ) : null}

          <AlertDialogFooter>
            <AlertDialogCancel disabled={quarantine.isPending}>
              Cancel
            </AlertDialogCancel>
            <AlertDialogAction
              disabled={quarantine.isPending}
              onClick={(event) => {
                event.preventDefault();
                quarantine.mutate();
              }}
            >
              {quarantine.isPending ? "Preserving..." : "Preserve and continue"}
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </>
  );
}
