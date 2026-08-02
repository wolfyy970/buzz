export type AgentProjectScope = {
  relayUrl: string;
  operatorPubkey: string;
  /** Durable NIP-34 repository coordinate. Local Project.id is never persisted. */
  repoAddress: string;
  /** Project discussion channel used to scope agent traffic and tool access. */
  channelId: string;
};

export type AgentToolRequirement = {
  /** Stable template-local key used by instance bindings. */
  id: string;
  /** Plain-language name shown in the product. */
  label: string;
  /** Stable capability ID, for example `mcp.tool.run_report`. */
  capability: string;
  required: boolean;
};
