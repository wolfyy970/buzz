import { listen } from "@tauri-apps/api/event";

import { invokeTauri } from "@/shared/api/tauri";
import type {
  AgentProjectScope,
  AgentTemplateVersionRef,
  AgentToolRequirement,
  RespondToMode,
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
  instructionChange: AgentTemplateInstructionChange | null;
  toolChanges: AgentTemplateToolChanges;
  skillChanges: AgentTemplateSkillChanges;
  versionChanges: AgentTemplateVersionChanges;
  overridesPreserved: AgentTemplateOverridesPreserved;
  toolBindingIssues: string[];
};

export type AgentTemplateInstructionChange = {
  before: string;
  after: string;
  privateOverridePreserved: boolean;
};

export type AgentTemplateToolChanges = {
  added: AgentToolRequirement[];
  changed: Array<{
    before: AgentToolRequirement;
    after: AgentToolRequirement;
    labelChanged: boolean;
    capabilityChanged: boolean;
    requiredChanged: boolean;
  }>;
  removed: AgentToolRequirement[];
};

export type AgentTemplateSkillChanges = {
  added: AgentSkill[];
  changed: Array<{
    before: AgentSkill;
    after: AgentSkill;
    fileChanges: AgentTemplateSkillFileChanges;
  }>;
  removed: AgentSkill[];
};

export type AgentTemplateSkillFileContent = {
  path: string;
  content: string;
};

export type AgentTemplateSkillFileContentChange = {
  path: string;
  before: string;
  after: string;
};

export type AgentTemplateSkillFileChanges = {
  added: AgentTemplateSkillFileContent[];
  changed: AgentTemplateSkillFileContentChange[];
  removed: AgentTemplateSkillFileContent[];
};

export type AgentTemplateOptionalStringChange = {
  before: string | null;
  after: string | null;
};

export type AgentTemplateParallelismChange = {
  before: number;
  after: number;
};

export type AgentTemplateAccessChange = {
  before: RespondToMode;
  after: RespondToMode;
  allowlistAdded: string[];
  allowlistRemoved: string[];
};

export type AgentTemplateEnvironmentChanges = {
  addedKeys: string[];
  changedKeys: string[];
  removedKeys: string[];
};

export type AgentTemplateVersionChanges = {
  runtime: AgentTemplateOptionalStringChange | null;
  provider: AgentTemplateOptionalStringChange | null;
  model: AgentTemplateOptionalStringChange | null;
  access: AgentTemplateAccessChange | null;
  parallelism: AgentTemplateParallelismChange | null;
  environment: AgentTemplateEnvironmentChanges;
};

export type AgentTemplateOverridesPreserved = {
  instructions: boolean;
  runtime: boolean;
  model: boolean;
  provider: boolean;
  skills: boolean;
  localEnvironment: boolean;
  localEnvironmentKeys: string[];
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

export type AgentTemplateUpdateRecoveryStage =
  | "prepared"
  | "before_original_handoff"
  | "after_original_handoff"
  | "before_candidate_commit"
  | "after_candidate_commit"
  | "before_candidate_launch"
  | "after_candidate_ready"
  | "before_candidate_handoff"
  | "after_candidate_handoff"
  | "before_original_restore"
  | "after_original_restore"
  | "before_original_restart"
  | "after_original_restart"
  | "update_completed"
  | "rollback_completed";

export type AgentTemplateUpdateRecoveryStatus = {
  transactionId: string | null;
  templateId: string | null;
  stage: AgentTemplateUpdateRecoveryStage | null;
  recovery: string;
  agents: Array<{
    pubkey: string;
    name: string;
  }>;
  requiresAttention: boolean;
  detail: string;
};

function fallbackSkillFileChanges(
  before: AgentSkill,
  after: AgentSkill,
): AgentTemplateSkillFileChanges {
  const beforeByPath = new Map(before.files.map((file) => [file.path, file]));
  const afterByPath = new Map(after.files.map((file) => [file.path, file]));
  return {
    added: after.files.filter((file) => !beforeByPath.has(file.path)),
    changed: after.files.flatMap((file) => {
      const previous = beforeByPath.get(file.path);
      return previous && previous.content !== file.content
        ? [
            {
              path: file.path,
              before: previous.content,
              after: file.content,
            },
          ]
        : [];
    }),
    removed: before.files.filter((file) => !afterByPath.has(file.path)),
  };
}

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
  type RawTarget = Omit<
    AgentTemplateUpdateTarget,
    | "projectScope"
    | "connectionBindings"
    | "instructionChange"
    | "toolChanges"
    | "skillChanges"
    | "versionChanges"
    | "overridesPreserved"
    | "toolBindingIssues"
  > & {
    projectScope?: AgentProjectScope | null;
    connectionBindings?: Record<string, string>;
    instructionChange?: AgentTemplateInstructionChange | null;
    toolChanges?: AgentTemplateToolChanges;
    skillChanges?: AgentTemplateSkillChanges;
    versionChanges?: AgentTemplateVersionChanges;
    overridesPreserved?: AgentTemplateOverridesPreserved;
    toolBindingIssues?: string[];
  };
  const preview = await invokeTauri<
    Omit<
      AgentTemplateUpdatePreview,
      "targetToolRequirements" | "targetSkills" | "agents"
    > & {
      targetToolRequirements?: AgentToolRequirement[];
      targetSkills?: AgentSkill[];
      agents: RawTarget[];
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
      instructionChange: agent.instructionChange ?? null,
      toolChanges: {
        added: agent.toolChanges?.added ?? [],
        changed: (agent.toolChanges?.changed ?? []).map((change) => ({
          ...change,
          labelChanged:
            change.labelChanged ?? change.before.label !== change.after.label,
          capabilityChanged:
            change.capabilityChanged ??
            change.before.capability !== change.after.capability,
          requiredChanged:
            change.requiredChanged ??
            change.before.required !== change.after.required,
        })),
        removed: agent.toolChanges?.removed ?? [],
      },
      skillChanges: {
        added: agent.skillChanges?.added ?? [],
        changed: (agent.skillChanges?.changed ?? []).map((change) => ({
          ...change,
          fileChanges:
            change.fileChanges ??
            fallbackSkillFileChanges(change.before, change.after),
        })),
        removed: agent.skillChanges?.removed ?? [],
      },
      versionChanges: agent.versionChanges ?? {
        runtime: null,
        provider: null,
        model: null,
        access: null,
        parallelism: null,
        environment: {
          addedKeys: [],
          changedKeys: [],
          removedKeys: [],
        },
      },
      overridesPreserved: agent.overridesPreserved ?? {
        instructions: false,
        runtime: false,
        model: false,
        provider: false,
        skills: false,
        localEnvironment: false,
        localEnvironmentKeys: [],
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

export async function listAgentTemplateUpdateRecoveries(): Promise<
  AgentTemplateUpdateRecoveryStatus[]
> {
  return invokeTauri<AgentTemplateUpdateRecoveryStatus[]>(
    "list_agent_template_update_recoveries",
  );
}

export async function restoreInterruptedAgentTemplateUpdate(
  transactionId: string,
): Promise<void> {
  return invokeTauri<void>("restore_interrupted_agent_template_update", {
    input: {
      transactionId,
      confirmStopBuzzOwnedAgents: true,
    },
  });
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
