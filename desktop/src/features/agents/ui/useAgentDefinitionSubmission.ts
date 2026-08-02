import * as React from "react";

import type { AgentDefinitionDialogProps } from "./AgentDefinitionDialogTypes";

export type AgentDefinitionSubmitAction = "save" | "publish";

export function useAgentDefinitionSubmission(
  onSubmit: AgentDefinitionDialogProps["onSubmit"],
) {
  const [pendingAction, setPendingAction] =
    React.useState<AgentDefinitionSubmitAction | null>(null);

  async function submit(
    input: Parameters<typeof onSubmit>[0],
    action: AgentDefinitionSubmitAction,
    publishCatalogUpdates: boolean,
  ) {
    setPendingAction(action);
    try {
      await onSubmit(input, {
        publishCatalogUpdates,
        publishTemplateVersion: action === "publish",
      });
    } finally {
      setPendingAction(null);
    }
  }

  return { pendingAction, submit };
}
