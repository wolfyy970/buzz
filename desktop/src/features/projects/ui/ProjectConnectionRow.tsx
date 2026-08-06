import {
  KeyRound,
  LoaderCircle,
  Pencil,
  RefreshCw,
  Trash2,
  Wrench,
} from "lucide-react";
import * as React from "react";

import type {
  ProjectConnection,
  ProjectConnectionHealthStatus,
} from "@/shared/api/tauriProjectConnections";
import { Button } from "@/shared/ui/button";

const HEALTH_COPY: Record<
  ProjectConnectionHealthStatus,
  { label: string; className: string }
> = {
  ready: {
    label: "Tools found",
    className: "bg-emerald-500/10 text-emerald-700 dark:text-emerald-300",
  },
  not_tested: {
    label: "Not tested",
    className: "bg-muted text-muted-foreground",
  },
  check_needed: {
    label: "Check needed",
    className: "bg-amber-500/10 text-amber-700 dark:text-amber-300",
  },
  approval_required: {
    label: "Approval required",
    className: "bg-amber-500/10 text-amber-700 dark:text-amber-300",
  },
  sign_in_required: {
    label: "Sign-in required",
    className: "bg-amber-500/10 text-amber-700 dark:text-amber-300",
  },
  missing_access: {
    label: "Missing access",
    className: "bg-amber-500/10 text-amber-700 dark:text-amber-300",
  },
  unavailable: {
    label: "Unavailable",
    className: "bg-destructive/10 text-destructive",
  },
};

const TOOL_PREVIEW_LIMIT = 4;

function toolLabel(tool: string) {
  return tool
    .replaceAll("_", " ")
    .replace(/\b\w/g, (letter) => letter.toUpperCase());
}

export function projectConnectionNeedsEditing(
  connection: ProjectConnection,
): boolean {
  return (
    connection.health.status === "sign_in_required" ||
    connection.health.status === "missing_access" ||
    connection.health.status === "approval_required" ||
    (connection.health.status === "unavailable" &&
      connection.health.detail === "Buzz could not start this MCP server.")
  );
}

function connectionActionLabel(connection: ProjectConnection) {
  if (connection.health.status === "sign_in_required") {
    return "Update sign-in";
  }
  if (connection.health.status === "missing_access") {
    return "Update credentials";
  }
  if (connection.health.status === "approval_required") {
    return "Review command";
  }
  if (
    connection.health.status === "unavailable" &&
    connection.health.detail === "Buzz could not start this MCP server."
  ) {
    return "Review setup";
  }
  return connection.health.status === "not_tested" ? "Test" : "Test again";
}

function formatVerificationTime(timestamp: string | null) {
  if (!timestamp) return "Never tested";
  const date = new Date(timestamp);
  return Number.isNaN(date.valueOf())
    ? "Last test unavailable"
    : `Tested ${date.toLocaleString()}`;
}

function ConnectionHealthBadge({
  status,
}: {
  status: ProjectConnectionHealthStatus;
}) {
  const copy = HEALTH_COPY[status];
  return (
    <span
      className={`inline-flex items-center rounded-full px-2 py-0.5 text-2xs font-medium ${copy.className}`}
    >
      {copy.label}
    </span>
  );
}

export function ProjectConnectionRow({
  automaticTestInterrupted,
  connection,
  onEdit,
  onRemove,
  onTest,
  testPending,
  testing,
}: {
  automaticTestInterrupted: boolean;
  connection: ProjectConnection;
  onEdit: () => void;
  onRemove: () => void;
  onTest: () => void;
  testPending: boolean;
  testing: boolean;
}) {
  const recoveryNeedsEdit = projectConnectionNeedsEditing(connection);
  const [showAllTools, setShowAllTools] = React.useState(false);
  const visibleTools = showAllTools
    ? connection.discoveredTools
    : connection.discoveredTools.slice(0, TOOL_PREVIEW_LIMIT);
  const hiddenToolCount = Math.max(
    0,
    connection.discoveredTools.length - TOOL_PREVIEW_LIMIT,
  );
  const actionLabel = connectionActionLabel(connection);

  return (
    <div
      aria-busy={testing || undefined}
      className="flex min-w-0 flex-wrap items-start gap-3 px-4 py-3"
      data-testid={`project-connection-${connection.id}`}
    >
      <Wrench className="mt-0.5 h-4 w-4 shrink-0 text-muted-foreground" />
      <div className="min-w-0 flex-1 basis-64 space-y-2">
        <div className="flex min-w-0 flex-wrap items-center gap-2">
          <p
            className="min-w-0 truncate text-sm font-medium text-foreground"
            title={connection.name}
          >
            {connection.name}
          </p>
          <ConnectionHealthBadge status={connection.health.status} />
        </div>
        <div className="space-y-0.5">
          <p
            className="truncate text-xs text-muted-foreground"
            title={connection.provider}
          >
            {connection.provider}
          </p>
          <p
            aria-live="polite"
            className="text-xs text-muted-foreground"
            role="status"
          >
            {testing
              ? `Testing ${connection.name}…`
              : formatVerificationTime(connection.health.lastVerifiedAt)}
          </p>
          {connection.health.detail ? (
            <p className="text-xs text-muted-foreground">
              {connection.health.detail}
            </p>
          ) : null}
        </div>
        {automaticTestInterrupted ? (
          <div
            className="rounded-lg bg-amber-500/10 px-3 py-2 text-xs text-foreground"
            data-testid={`project-connection-test-interrupted-${connection.id}`}
            role="status"
          >
            <p className="font-medium">Automatic test interrupted</p>
            <p className="mt-0.5 text-muted-foreground">
              Setup was saved, but its automatic test did not run. Test this
              connection to finish setup.
            </p>
          </div>
        ) : null}
        {visibleTools.length > 0 ? (
          <div className="flex flex-wrap items-center gap-1.5">
            {visibleTools.map((tool) => (
              <span
                className="max-w-48 truncate rounded-md bg-muted px-2 py-1 text-2xs text-muted-foreground"
                key={tool}
                title={tool}
              >
                {toolLabel(tool)}
              </span>
            ))}
            {hiddenToolCount > 0 ? (
              <Button
                aria-expanded={showAllTools}
                className="h-6 px-1.5 text-2xs"
                onClick={() => setShowAllTools((value) => !value)}
                size="xs"
                type="button"
                variant="ghost"
              >
                {showAllTools ? "Show fewer" : `Show ${hiddenToolCount} more`}
              </Button>
            ) : null}
          </div>
        ) : (
          <p className="text-xs text-muted-foreground">
            {connection.health.status === "not_tested"
              ? "Test this connection to discover its tools."
              : "No tools are currently available."}
          </p>
        )}
      </div>
      <div className="ml-7 flex shrink-0 items-center gap-1 sm:ml-0">
        <Button
          aria-label={
            testing
              ? `Testing ${connection.name}`
              : `${actionLabel} ${connection.name}`
          }
          disabled={testPending}
          onClick={recoveryNeedsEdit ? onEdit : onTest}
          size="sm"
          variant="outline"
        >
          {testing ? (
            <LoaderCircle className="h-4 w-4 animate-spin" />
          ) : recoveryNeedsEdit ? (
            <KeyRound className="h-4 w-4" />
          ) : (
            <RefreshCw className="h-4 w-4" />
          )}
          {testing ? "Testing…" : actionLabel}
        </Button>
        <Button
          aria-label={`Edit ${connection.name}`}
          disabled={testing}
          onClick={onEdit}
          size="icon-xs"
          title={`Edit ${connection.name}`}
          variant="ghost"
        >
          <Pencil className="h-4 w-4" />
        </Button>
        <Button
          aria-label={`Remove ${connection.name}`}
          disabled={testing}
          onClick={onRemove}
          size="icon-xs"
          title={`Remove ${connection.name}`}
          variant="ghost"
        >
          <Trash2 className="h-4 w-4" />
        </Button>
      </div>
    </div>
  );
}
