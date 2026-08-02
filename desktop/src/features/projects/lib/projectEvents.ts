import type { RelayEvent } from "@/shared/api/types";

export function dedupProjectEvents(events: RelayEvent[]): RelayEvent[] {
  const best = new Map<string, RelayEvent>();
  for (const event of events) {
    const dtag = event.tags.find((tag) => tag[0] === "d")?.[1] ?? "";
    const key = `${event.pubkey}:${event.kind}:${dtag}`;
    const previous = best.get(key);
    if (!previous || event.created_at > previous.created_at) {
      best.set(key, event);
    }
  }
  return [...best.values()];
}
