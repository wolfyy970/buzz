import type {
  AgentProjectScope,
  AgentToolRequirement,
} from "./agentProjectTypes";

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
  avatarUrl: string | null;
  model: string | null;
  modelSource: "definition" | "global" | "instance_legacy" | null;
  provider: string | null;
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
  connectionBindings: Record<string, string>;
  respondTo: RespondToMode;
  respondToAllowlist: string[];
};
