import * as React from "react";

import {
  consumePendingOpenEditAgent,
  type EditAgentFocusTarget,
  subscribeOpenEditAgent,
} from "@/features/agents/openEditAgentEvent";
import { AgentDialog } from "@/features/agents/ui/AgentDialog";
import { AgentEditScopeDialog } from "@/features/agents/ui/AgentEditScopeDialog";
import type { AgentPersona, ManagedAgent } from "@/shared/api/types";

export type UserProfileAgentEditDialogsController = {
  closeInstance: () => void;
  closeScope: () => void;
  focus: EditAgentFocusTarget | undefined;
  instanceOpen: boolean;
  onInstanceOpenChange: (open: boolean) => void;
  onScopeOpenChange: (open: boolean) => void;
  openInstance: () => void;
  openScope: () => void;
  scopeOpen: boolean;
};

export function useUserProfileAgentEditDialogs(
  pubkey: string | undefined,
): UserProfileAgentEditDialogsController {
  const [instanceOpen, setInstanceOpen] = React.useState(false);
  const [scopeOpen, setScopeOpen] = React.useState(false);
  const [focus, setFocus] = React.useState<EditAgentFocusTarget | undefined>(
    undefined,
  );

  React.useEffect(() => {
    if (!pubkey) return;
    const pending = consumePendingOpenEditAgent(pubkey);
    if (pending !== false) {
      setFocus(pending === true ? undefined : pending);
      setInstanceOpen(true);
    }
    return subscribeOpenEditAgent(pubkey, (nextFocus) => {
      setFocus(nextFocus);
      setInstanceOpen(true);
    });
  }, [pubkey]);

  const closeInstance = React.useCallback(() => {
    setInstanceOpen(false);
    setFocus(undefined);
  }, []);
  const closeScope = React.useCallback(() => setScopeOpen(false), []);
  const onInstanceOpenChange = React.useCallback((open: boolean) => {
    setInstanceOpen(open);
    if (!open) setFocus(undefined);
  }, []);
  const onScopeOpenChange = React.useCallback(
    (open: boolean) => setScopeOpen(open),
    [],
  );
  const openInstance = React.useCallback(() => setInstanceOpen(true), []);
  const openScope = React.useCallback(() => setScopeOpen(true), []);

  return {
    closeInstance,
    closeScope,
    focus,
    instanceOpen,
    onInstanceOpenChange,
    onScopeOpenChange,
    openInstance,
    openScope,
    scopeOpen,
  };
}

export function UserProfileAgentEditDialogs({
  affectedAgents,
  agent,
  controller,
  onEditTemplate,
  persona,
}: {
  affectedAgents: ManagedAgent[];
  agent: ManagedAgent;
  controller: UserProfileAgentEditDialogsController;
  onEditTemplate: (persona: AgentPersona) => void;
  persona: AgentPersona | undefined;
}) {
  return (
    <>
      {persona ? (
        <AgentEditScopeDialog
          affectedAgents={affectedAgents}
          agent={agent}
          onEditInstance={() => {
            controller.closeScope();
            controller.openInstance();
          }}
          onEditTemplate={() => {
            controller.closeScope();
            onEditTemplate(persona);
          }}
          onOpenChange={controller.onScopeOpenChange}
          open={controller.scopeOpen}
          persona={persona}
        />
      ) : null}
      <AgentDialog
        agent={agent}
        initialFocus={controller.focus}
        mode="instance-edit"
        onEditLinkedPersona={
          persona && !persona.isBuiltIn
            ? () => {
                controller.closeInstance();
                onEditTemplate(persona);
              }
            : undefined
        }
        onOpenChange={controller.onInstanceOpenChange}
        open={controller.instanceOpen}
      />
    </>
  );
}
