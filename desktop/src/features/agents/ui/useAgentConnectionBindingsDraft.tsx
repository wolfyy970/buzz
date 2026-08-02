import * as React from "react";

import type { ManagedAgent } from "@/shared/api/types";
import { AgentConnectionBindingsSection } from "./AgentConnectionBindingsSection";

function recordsEqual(
  left: Record<string, string>,
  right: Record<string, string>,
) {
  const leftEntries = Object.entries(left);
  return (
    leftEntries.length === Object.keys(right).length &&
    leftEntries.every(([key, value]) => right[key] === value)
  );
}

export function useAgentConnectionBindingsDraft({
  agent,
  disabled,
  open,
}: {
  agent: ManagedAgent;
  disabled: boolean;
  open: boolean;
}) {
  const [bindings, setBindings] = React.useState(agent.connectionBindings);
  const [valid, setValid] = React.useState(
    !agent.toolRequirements.some((requirement) => requirement.required),
  );

  // biome-ignore lint/correctness/useExhaustiveDependencies: polling must not wipe in-dialog edits
  React.useEffect(() => {
    if (!open) return;
    setBindings(agent.connectionBindings);
    setValid(
      !agent.toolRequirements.some((requirement) => requirement.required),
    );
  }, [agent.pubkey, open]);

  const handleValidityChange = React.useCallback(
    (nextValid: boolean) => setValid(nextValid),
    [],
  );

  return {
    valid,
    update: recordsEqual(bindings, agent.connectionBindings)
      ? undefined
      : bindings,
    section: (
      <AgentConnectionBindingsSection
        bindings={bindings}
        disabled={disabled}
        onBindingsChange={setBindings}
        onValidityChange={handleValidityChange}
        projectScope={agent.projectScope}
        requirements={agent.toolRequirements}
      />
    ),
  };
}
