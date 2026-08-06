import { getRelayWsUrl } from "@/shared/api/tauri";

export async function assertExpectedRelay(
  expectedRelayUrl: string | undefined,
  connectedRelayUrl: string | null,
): Promise<void> {
  if (!expectedRelayUrl) return;
  const configuredRelayUrl = await getRelayWsUrl();
  if (
    configuredRelayUrl !== expectedRelayUrl ||
    (connectedRelayUrl !== null && connectedRelayUrl !== expectedRelayUrl)
  ) {
    throw new Error(
      "The active community changed before the Project could be deleted.",
    );
  }
}
