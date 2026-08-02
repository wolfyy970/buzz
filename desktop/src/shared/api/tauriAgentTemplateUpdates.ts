import { invokeTauri } from "@/shared/api/tauri";

export type AgentTemplateUpdateTarget = {
  pubkey: string;
  name: string;
  currentVersion: string | null;
  targetVersion: string;
  runningRelays: string[];
  eligible: boolean;
  blockedReason: string | null;
};

export type AgentTemplateUpdatePreview = {
  personaId: string;
  personaName: string;
  targetVersion: string;
  agents: AgentTemplateUpdateTarget[];
};

export type AgentTemplateUpdateOutcome =
  | "updated"
  | "updated_stopped"
  | "rolled_back"
  | "rollback_failed";

export type AgentTemplateUpdateResult = {
  pubkey: string;
  name: string;
  outcome: AgentTemplateUpdateOutcome;
  error: string | null;
};

export type ApplyAgentTemplateUpdateResponse = {
  personaId: string;
  version: string;
  rolledBack: boolean;
  agents: AgentTemplateUpdateResult[];
};

export async function previewAgentTemplateUpdate(
  personaId: string,
): Promise<AgentTemplateUpdatePreview> {
  return invokeTauri<AgentTemplateUpdatePreview>(
    "preview_agent_template_update",
    { personaId },
  );
}

export async function applyAgentTemplateUpdate(input: {
  personaId: string;
  expectedVersion: string;
  selectedPubkeys: string[];
}): Promise<ApplyAgentTemplateUpdateResponse> {
  return invokeTauri<ApplyAgentTemplateUpdateResponse>(
    "apply_agent_template_update",
    { input },
  );
}
