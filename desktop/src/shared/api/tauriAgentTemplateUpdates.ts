import { invokeTauri } from "@/shared/api/tauri";
import type {
  AgentProjectScope,
  AgentToolRequirement,
} from "@/shared/api/types";

export type AgentTemplateUpdateTarget = {
  pubkey: string;
  name: string;
  currentVersion: string | null;
  targetVersion: string;
  runningRelays: string[];
  eligible: boolean;
  blockedReason: string | null;
  projectScope: AgentProjectScope | null;
  connectionBindings: Record<string, string>;
  toolChanges: AgentTemplateToolChanges;
  toolBindingIssues: string[];
};

export type AgentTemplateToolChanges = {
  added: AgentToolRequirement[];
  changed: Array<{
    before: AgentToolRequirement;
    after: AgentToolRequirement;
  }>;
  removed: AgentToolRequirement[];
};

export type AgentTemplateUpdatePreview = {
  personaId: string;
  personaName: string;
  targetVersion: string;
  targetToolRequirements: AgentToolRequirement[];
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
  const preview = await invokeTauri<
    Omit<AgentTemplateUpdatePreview, "targetToolRequirements"> & {
      targetToolRequirements?: AgentToolRequirement[];
    }
  >("preview_agent_template_update", { personaId });
  return {
    ...preview,
    targetToolRequirements: preview.targetToolRequirements ?? [],
    agents: preview.agents.map((agent) => ({
      ...agent,
      projectScope: agent.projectScope ?? null,
      connectionBindings: agent.connectionBindings ?? {},
      toolChanges: agent.toolChanges ?? {
        added: [],
        changed: [],
        removed: [],
      },
      toolBindingIssues: agent.toolBindingIssues ?? [],
    })),
  };
}

export async function applyAgentTemplateUpdate(input: {
  personaId: string;
  expectedVersion: string;
  selectedPubkeys: string[];
  connectionBindingsByPubkey: Record<string, Record<string, string>>;
}): Promise<ApplyAgentTemplateUpdateResponse> {
  return invokeTauri<ApplyAgentTemplateUpdateResponse>(
    "apply_agent_template_update",
    { input },
  );
}
