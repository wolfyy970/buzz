import { invokeTauri } from "@/shared/api/tauri";
import type {
  AgentProjectScope,
  AgentTemplateVersionRef,
  AgentToolRequirement,
} from "@/shared/api/types";
import type { AgentSkill } from "@/shared/api/agentSkillTypes";

export type AgentTemplateUpdateTarget = {
  pubkey: string;
  name: string;
  currentVersion: string | null;
  targetVersion: AgentTemplateVersionRef;
  runningRelays: string[];
  eligible: boolean;
  blockedReason: string | null;
  projectScope: AgentProjectScope | null;
  connectionBindings: Record<string, string>;
  toolChanges: AgentTemplateToolChanges;
  skillChanges: AgentTemplateSkillChanges;
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

export type AgentTemplateSkillChanges = {
  added: AgentSkill[];
  changed: Array<{
    before: AgentSkill;
    after: AgentSkill;
  }>;
  removed: AgentSkill[];
};

export type AgentTemplateUpdatePreview = {
  personaId: string;
  personaName: string;
  targetVersion: AgentTemplateVersionRef;
  targetVersionToken: string;
  targetToolRequirements: AgentToolRequirement[];
  targetSkills: AgentSkill[];
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
  version: AgentTemplateVersionRef;
  rolledBack: boolean;
  agents: AgentTemplateUpdateResult[];
};

export async function previewAgentTemplateUpdate(
  personaId: string,
  targetVersion?: AgentTemplateVersionRef,
): Promise<AgentTemplateUpdatePreview> {
  const preview = await invokeTauri<
    Omit<
      AgentTemplateUpdatePreview,
      "targetToolRequirements" | "targetSkills"
    > & {
      targetToolRequirements?: AgentToolRequirement[];
      targetSkills?: AgentSkill[];
    }
  >("preview_agent_template_update", { personaId, targetVersion });
  return {
    ...preview,
    targetToolRequirements: preview.targetToolRequirements ?? [],
    targetSkills: preview.targetSkills ?? [],
    agents: preview.agents.map((agent) => ({
      ...agent,
      projectScope: agent.projectScope ?? null,
      connectionBindings: agent.connectionBindings ?? {},
      toolChanges: agent.toolChanges ?? {
        added: [],
        changed: [],
        removed: [],
      },
      skillChanges: agent.skillChanges ?? {
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
  expectedVersion: AgentTemplateVersionRef;
  selectedPubkeys: string[];
  connectionBindingsByPubkey: Record<string, Record<string, string>>;
}): Promise<ApplyAgentTemplateUpdateResponse> {
  return invokeTauri<ApplyAgentTemplateUpdateResponse>(
    "apply_agent_template_update",
    { input },
  );
}

export type PublishAgentTemplateVersionResult = {
  personaId: string;
  personaName: string;
  version: AgentTemplateVersionRef;
};

export function publishAgentTemplateVersionPayload(input: {
  personaId: string;
  expectedUpdatedAt: string;
}) {
  return {
    input: {
      personaId: input.personaId,
      expectedUpdatedAt: input.expectedUpdatedAt,
    },
  };
}

export async function publishAgentTemplateVersion(input: {
  personaId: string;
  expectedUpdatedAt: string;
}): Promise<PublishAgentTemplateVersionResult> {
  return invokeTauri<PublishAgentTemplateVersionResult>(
    "publish_agent_template_version",
    publishAgentTemplateVersionPayload(input),
  );
}
