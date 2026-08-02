import * as React from "react";
import { toast } from "sonner";

import { hasOutdatedAgentTemplateInstances } from "@/features/agents/lib/agentTemplateUpdatePreview";
import { AgentTemplateUpdateDialog } from "@/features/agents/ui/AgentTemplateUpdateDialog";
import {
  applyAgentTemplateUpdate,
  publishAgentTemplateVersion,
  previewAgentTemplateUpdate,
  type AgentTemplateUpdateProgressStage,
  type AgentTemplateUpdatePreview,
  type ApplyAgentTemplateUpdateResponse,
  type PublishAgentTemplateVersionResult,
} from "@/shared/api/tauriAgentTemplateUpdates";
import type { AgentPersona } from "@/shared/api/types";
import { useProfilePanel } from "@/shared/context/ProfilePanelContext";

export type ProfileAgentTemplateUpdateController = {
  apply: (
    selectedPubkeys: string[],
    connectionBindingsByPubkey: Record<string, Record<string, string>>,
  ) => Promise<void>;
  dialogOpen: boolean;
  error: string | null;
  isPending: boolean;
  onOpenChange: (open: boolean) => void;
  preview: AgentTemplateUpdatePreview | null;
  progressStage: AgentTemplateUpdateProgressStage | null;
  result: ApplyAgentTemplateUpdateResponse | null;
  publishSavedTemplate: (persona: AgentPersona) => Promise<void>;
};

export function useProfileAgentTemplateUpdate({
  onAgentsUpdated,
}: {
  onAgentsUpdated: () => unknown;
}): ProfileAgentTemplateUpdateController {
  const [preview, setPreview] =
    React.useState<AgentTemplateUpdatePreview | null>(null);
  const [dialogOpen, setDialogOpen] = React.useState(false);
  const dialogOpenRef = React.useRef(false);
  const [result, setResult] =
    React.useState<ApplyAgentTemplateUpdateResponse | null>(null);
  const [error, setError] = React.useState<string | null>(null);
  const [isPending, setIsPending] = React.useState(false);
  const [progressStage, setProgressStage] =
    React.useState<AgentTemplateUpdateProgressStage | null>(null);

  const publishSavedTemplate = React.useCallback(
    async (persona: AgentPersona) => {
      let published: PublishAgentTemplateVersionResult;
      try {
        published = await publishAgentTemplateVersion({
          personaId: persona.id,
          expectedUpdatedAt: persona.updatedAt,
        });
      } catch {
        toast.error("Template saved. Version wasn’t published. Try again.");
        return;
      }
      toast.success(`Published a new version of ${published.personaName}.`);
      try {
        const nextPreview = await previewAgentTemplateUpdate(
          published.personaId,
          published.version,
        );
        if (hasOutdatedAgentTemplateInstances(nextPreview)) {
          setResult(null);
          setError(null);
          setProgressStage(null);
          setPreview(nextPreview);
          dialogOpenRef.current = true;
          setDialogOpen(true);
        }
      } catch (previewError) {
        toast.error(
          previewError instanceof Error
            ? `${published.personaName} was published, but Buzz could not load the affected agents: ${previewError.message}`
            : `${published.personaName} was published, but Buzz could not load the affected agents.`,
        );
      }
    },
    [],
  );

  const apply = React.useCallback(
    async (
      selectedPubkeys: string[],
      connectionBindingsByPubkey: Record<string, Record<string, string>>,
    ) => {
      if (!preview || isPending) return;
      setError(null);
      setResult(null);
      setProgressStage("preparing_update");
      setIsPending(true);
      try {
        const nextResult = await applyAgentTemplateUpdate(
          {
            personaId: preview.personaId,
            expectedVersion: preview.targetVersion,
            selectedPubkeys,
            connectionBindingsByPubkey,
          },
          {
            onProgress: ({ stage }) => setProgressStage(stage),
          },
        );
        setResult(nextResult);
        void onAgentsUpdated();
        if (!dialogOpenRef.current) {
          const needsAttention = nextResult.agents.some(
            (agent) => agent.outcome === "rollback_failed",
          );
          const options = {
            action: {
              label: "View results",
              onClick: () => {
                dialogOpenRef.current = true;
                setDialogOpen(true);
              },
            },
            duration: needsAttention ? Number.POSITIVE_INFINITY : 10_000,
          };
          if (needsAttention) {
            toast.error("Some agents need attention.", options);
          } else if (nextResult.rolledBack) {
            toast.warning(
              "Update rolled back. Previous version restored.",
              options,
            );
          } else {
            toast.success(
              `${nextResult.agents.length} ${
                nextResult.agents.length === 1 ? "agent" : "agents"
              } updated.`,
              options,
            );
          }
        }
      } catch (applyError) {
        const message =
          applyError instanceof Error
            ? applyError.message
            : "The agents were not updated.";
        setError(message);
        if (!dialogOpenRef.current) {
          toast.error("Agent update failed.", {
            action: {
              label: "View details",
              onClick: () => {
                dialogOpenRef.current = true;
                setDialogOpen(true);
              },
            },
            description: message,
            duration: Number.POSITIVE_INFINITY,
          });
        }
      } finally {
        setIsPending(false);
      }
    },
    [isPending, onAgentsUpdated, preview],
  );

  const onOpenChange = React.useCallback(
    (open: boolean) => {
      dialogOpenRef.current = open;
      setDialogOpen(open);
      if (open || isPending) return;
      setPreview(null);
      setResult(null);
      setError(null);
      setProgressStage(null);
    },
    [isPending],
  );

  return {
    apply,
    dialogOpen,
    error,
    isPending,
    onOpenChange,
    preview,
    progressStage,
    result,
    publishSavedTemplate,
  };
}

export function UserProfileAgentTemplateUpdateDialog({
  controller,
}: {
  controller: ProfileAgentTemplateUpdateController;
}) {
  const { openProfilePanel } = useProfilePanel();
  return (
    <AgentTemplateUpdateDialog
      error={controller.error}
      isPending={controller.isPending}
      onApply={(selectedPubkeys, connectionBindingsByPubkey) => {
        void controller.apply(selectedPubkeys, connectionBindingsByPubkey);
      }}
      onOpenChange={controller.onOpenChange}
      onOpenAgent={(pubkey) => openProfilePanel?.(pubkey, { tab: "runtime" })}
      open={controller.dialogOpen}
      preview={controller.preview}
      progressStage={controller.progressStage}
      result={controller.result}
    />
  );
}
