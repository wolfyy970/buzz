export type AgentProjectScope = {
  relayUrl: string;
  operatorPubkey: string;
  /**
   * Durable NIP-MP Project coordinate. Legacy NIP-34 repository coordinates
   * remain valid for single-repository Projects.
   */
  projectAddress: string;
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
