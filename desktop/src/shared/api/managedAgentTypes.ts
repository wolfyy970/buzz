import type {
  AgentProjectScope,
  AgentToolRequirement,
} from "./agentProjectTypes";
import type { AgentSkill } from "./agentSkillTypes";

export type ManagedAgentBackend =
  | { type: "local" }
  | { type: "provider"; id: string; config: Record<string, unknown> };

export type RespondToMode = "owner-only" | "allowlist" | "anyone";

export type ManagedAgent = {
  pubkey: string;
  name: string;
  personaId: string | null;
  /** The record's harness ID. Null means it inherits from the template. */
  runtime: string | null;
  teamId?: string | null;
  relayUrl: string;
  acpCommand: string;
  agentCommand: string;
  agentCommandOverride: string | null;
  agentArgs: string[];
  mcpCommand: string;
  turnTimeoutSeconds: number;
  idleTimeoutSeconds: number | null;
  maxTurnDurationSeconds: number | null;
  parallelism: number;
  systemPrompt: string | null;
  instructionsChangedForAgent: boolean;
  avatarUrl: string | null;
  model: string | null;
  modelSource:
    | "definition"
    | "global"
    | "instance_override"
    | "instance_legacy"
    | null;
  modelChangedForAgent: boolean;
  provider: string | null;
  providerChangedForAgent: boolean;
  personaOutOfDate: boolean;
  personaOrphaned: boolean;
  needsRestart: boolean;
  envVars: Record<string, string>;
  status: "running" | "stopped" | "deployed" | "not_deployed";
  pid: number | null;
  createdAt: string;
  updatedAt: string;
  lastStartedAt: string | null;
  lastStoppedAt: string | null;
  lastExitCode: number | null;
  lastError: string | null;
  lastErrorCode: number | null;
  logPath: string;
  startOnAppLaunch: boolean;
  autoRestartOnConfigChange: boolean;
  backend: ManagedAgentBackend;
  backendAgentId: string | null;
  projectScope: AgentProjectScope | null;
  toolRequirements: AgentToolRequirement[];
  skills: AgentSkill[];
  skillsChangedForAgent: boolean;
  connectionBindings: Record<string, string>;
  respondTo: RespondToMode;
  respondToAllowlist: string[];
};
