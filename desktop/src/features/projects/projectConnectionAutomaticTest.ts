import type { ProjectConnection } from "@/shared/api/tauriProjectConnections";
import type { ProjectConnectionScope } from "@/shared/api/projectConnectionTypes";

const STORAGE_KEY = "buzz.projects.pending-connection-automatic-tests.v1";
const MAX_PENDING_TESTS = 64;

type PendingProjectConnectionAutomaticTest = {
  connectionId: string;
  projectScope: ProjectConnectionScope;
  savedAt: string;
};

function sameScope(
  left: ProjectConnectionScope,
  right: ProjectConnectionScope,
): boolean {
  return (
    left.relayUrl === right.relayUrl &&
    left.operatorPubkey === right.operatorPubkey &&
    left.projectAddress === right.projectAddress
  );
}

function isProjectConnectionScope(
  value: unknown,
): value is ProjectConnectionScope {
  if (!value || typeof value !== "object") return false;
  const candidate = value as Record<string, unknown>;
  return (
    typeof candidate.relayUrl === "string" &&
    candidate.relayUrl.length <= 2_048 &&
    typeof candidate.operatorPubkey === "string" &&
    candidate.operatorPubkey.length <= 128 &&
    typeof candidate.projectAddress === "string" &&
    candidate.projectAddress.length <= 4_096
  );
}

function isPendingAutomaticTest(
  value: unknown,
): value is PendingProjectConnectionAutomaticTest {
  if (!value || typeof value !== "object") return false;
  const candidate = value as Record<string, unknown>;
  return (
    typeof candidate.connectionId === "string" &&
    candidate.connectionId.length <= 256 &&
    isProjectConnectionScope(candidate.projectScope) &&
    typeof candidate.savedAt === "string" &&
    !Number.isNaN(Date.parse(candidate.savedAt))
  );
}

function readPendingAutomaticTests(): PendingProjectConnectionAutomaticTest[] {
  try {
    const parsed: unknown = JSON.parse(
      window.localStorage.getItem(STORAGE_KEY) ?? "[]",
    );
    if (!Array.isArray(parsed)) return [];
    return parsed.filter(isPendingAutomaticTest).slice(-MAX_PENDING_TESTS);
  } catch {
    return [];
  }
}

function writePendingAutomaticTests(
  pendingTests: PendingProjectConnectionAutomaticTest[],
): void {
  try {
    window.localStorage.setItem(
      STORAGE_KEY,
      JSON.stringify(pendingTests.slice(-MAX_PENDING_TESTS)),
    );
  } catch {
    // Connection persistence must still succeed if local interruption
    // bookkeeping is unavailable.
  }
}

export function recordPendingProjectConnectionAutomaticTest(
  connection: ProjectConnection,
): void {
  const current = readPendingAutomaticTests().filter(
    (candidate) =>
      candidate.connectionId !== connection.id ||
      !sameScope(candidate.projectScope, connection.projectScope),
  );
  writePendingAutomaticTests([
    ...current,
    {
      connectionId: connection.id,
      projectScope: connection.projectScope,
      savedAt: new Date().toISOString(),
    },
  ]);
}

export function clearPendingProjectConnectionAutomaticTest(
  projectScope: ProjectConnectionScope,
  connectionId: string,
): void {
  writePendingAutomaticTests(
    readPendingAutomaticTests().filter(
      (candidate) =>
        candidate.connectionId !== connectionId ||
        !sameScope(candidate.projectScope, projectScope),
    ),
  );
}

export function pendingProjectConnectionAutomaticTestIds(
  projectScope: ProjectConnectionScope,
): Set<string> {
  return new Set(
    readPendingAutomaticTests()
      .filter((candidate) => sameScope(candidate.projectScope, projectScope))
      .map((candidate) => candidate.connectionId),
  );
}
