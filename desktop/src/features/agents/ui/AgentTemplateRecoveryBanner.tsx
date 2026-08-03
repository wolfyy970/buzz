import { useMutation, useQuery } from "@tanstack/react-query";
import { AlertTriangle, RotateCcw } from "lucide-react";
import * as React from "react";

import {
  listAgentTemplateUpdateRecoveries,
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
    : "Agent template";
}

export function AgentTemplateRecoveryBanner({
  onRecovered,
  templateNamesById,
}: AgentTemplateRecoveryBannerProps) {
  const [selected, setSelected] =
    React.useState<AgentTemplateUpdateRecoveryStatus | null>(null);
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

  const attention = (recoveries.data ?? []).filter(
    (status) => status.requiresAttention,
  );
  const queryError =
    recoveries.error instanceof Error ? recoveries.error.message : null;
  const restoreError =
    restore.error instanceof Error ? restore.error.message : null;

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
                Buzz found an interrupted update. Agent starts and edits stay
                blocked until it is resolved.
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
              const name = templateName(status, templateNamesById);
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
                      {status.detail}
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
                    <p className="shrink-0 text-xs font-medium text-warning">
                      Manual review required
                    </p>
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
    </>
  );
}
