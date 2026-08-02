import { listen } from "@tauri-apps/api/event";

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

export type AgentTemplateUpdateProgressStage =
  | "preparing_update"
  | "finishing_current_task"
  | "starting_updated_agent"
  | "checking_update"
  | "updated"
  | "update_rolled_back"
  | "needs_attention";

export type AgentTemplateUpdateProgress = {
  requestId: string;
  stage: AgentTemplateUpdateProgressStage;
};

export const AGENT_TEMPLATE_UPDATE_PROGRESS_EVENT =
  "agent-template-update-progress";

export function agentTemplateUpdateProgressLabel(
  stage: AgentTemplateUpdateProgressStage,
): string {
  switch (stage) {
    case "preparing_update":
      return "Preparing update";
    case "finishing_current_task":
      return "Finishing current task";
    case "starting_updated_agent":
      return "Starting updated agent";
    case "checking_update":
      return "Checking update";
    case "updated":
      return "Updated";
    case "update_rolled_back":
      return "Update rolled back";
    case "needs_attention":
      return "Needs attention";
  }
}

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

export type ApplyAgentTemplateUpdateInput = {
  personaId: string;
  expectedVersion: AgentTemplateVersionRef;
  selectedPubkeys: string[];
  connectionBindingsByPubkey: Record<string, Record<string, string>>;
};

export function applyAgentTemplateUpdatePayload(
  input: ApplyAgentTemplateUpdateInput,
  requestId: string,
) {
  return { input: { requestId, ...input } };
}

export async function applyAgentTemplateUpdate(
  input: ApplyAgentTemplateUpdateInput,
  options?: {
    onProgress?: (progress: AgentTemplateUpdateProgress) => void;
  },
): Promise<ApplyAgentTemplateUpdateResponse> {
  const requestId = crypto.randomUUID();
  const unlisten = await listen<AgentTemplateUpdateProgress>(
    AGENT_TEMPLATE_UPDATE_PROGRESS_EVENT,
    (event) => {
      if (event.payload.requestId === requestId) {
        options?.onProgress?.(event.payload);
      }
    },
  );
  try {
    return await invokeTauri<ApplyAgentTemplateUpdateResponse>(
      "apply_agent_template_update",
      applyAgentTemplateUpdatePayload(input, requestId),
    );
  } finally {
    unlisten();
  }
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
