import * as React from "react";
import { toast } from "sonner";

import { hasOutdatedAgentTemplateInstances } from "@/features/agents/lib/agentTemplateUpdatePreview";
import { AgentTemplateUpdateDialog } from "@/features/agents/ui/AgentTemplateUpdateDialog";
import {
  applyAgentTemplateUpdate,
  publishAgentTemplateVersion,
  previewAgentTemplateUpdate,
  type AgentTemplateUpdatePreview,
  type ApplyAgentTemplateUpdateResponse,
  type PublishAgentTemplateVersionResult,
} from "@/shared/api/tauriAgentTemplateUpdates";
import type { AgentPersona } from "@/shared/api/types";

export type ProfileAgentTemplateUpdateController = {
  apply: (
    selectedPubkeys: string[],
    connectionBindingsByPubkey: Record<string, Record<string, string>>,
  ) => Promise<void>;
  error: string | null;
  isPending: boolean;
  onOpenChange: (open: boolean) => void;
  preview: AgentTemplateUpdatePreview | null;
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
  const [result, setResult] =
    React.useState<ApplyAgentTemplateUpdateResponse | null>(null);
  const [error, setError] = React.useState<string | null>(null);
  const [isPending, setIsPending] = React.useState(false);

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
          setPreview(nextPreview);
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
      setIsPending(true);
      try {
        const nextResult = await applyAgentTemplateUpdate({
          personaId: preview.personaId,
          expectedVersion: preview.targetVersion,
          selectedPubkeys,
          connectionBindingsByPubkey,
        });
        setResult(nextResult);
        void onAgentsUpdated();
      } catch (applyError) {
        setError(
          applyError instanceof Error
            ? applyError.message
            : "The agents were not updated.",
        );
      } finally {
        setIsPending(false);
      }
    },
    [isPending, onAgentsUpdated, preview],
  );

  const onOpenChange = React.useCallback(
    (open: boolean) => {
      if (open || isPending) return;
      setPreview(null);
      setResult(null);
      setError(null);
    },
    [isPending],
  );

  return {
    apply,
    error,
    isPending,
    onOpenChange,
    preview,
    result,
    publishSavedTemplate,
  };
}

export function UserProfileAgentTemplateUpdateDialog({
  controller,
}: {
  controller: ProfileAgentTemplateUpdateController;
}) {
  return (
    <AgentTemplateUpdateDialog
      error={controller.error}
      isPending={controller.isPending}
      onApply={(selectedPubkeys, connectionBindingsByPubkey) => {
        void controller.apply(selectedPubkeys, connectionBindingsByPubkey);
      }}
      onOpenChange={controller.onOpenChange}
      open={controller.preview !== null}
      preview={controller.preview}
      result={controller.result}
    />
  );
}
