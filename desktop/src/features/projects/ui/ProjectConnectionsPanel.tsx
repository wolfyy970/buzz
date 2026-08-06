import {
  ChevronDown,
  Link2,
  LoaderCircle,
  Plus,
  Trash2,
  XCircle,
} from "lucide-react";
import * as React from "react";
import { toast } from "sonner";

import { useCommunities } from "@/features/communities/useCommunities";
import {
  useCreateProjectConnectionMutation,
  useDeleteProjectConnectionMutation,
  useProjectConnectionsQuery,
  useTestProjectConnectionMutation,
  useUpdateProjectConnectionMutation,
} from "@/features/projects/projectConnectionHooks";
import {
  clearPendingProjectConnectionAutomaticTest,
  pendingProjectConnectionAutomaticTestIds,
  recordPendingProjectConnectionAutomaticTest,
} from "@/features/projects/projectConnectionAutomaticTest";
import type {
  ProjectConnection,
  ProjectConnectionDraft,
} from "@/shared/api/tauriProjectConnections";
import type { ProjectConnectionScope } from "@/shared/api/projectConnectionTypes";
import {
  markE2EProjectConnectionSettled,
  useMountedRef,
} from "./projectConnectionLifecycle";
import {
  ProjectConnectionRow,
  projectConnectionNeedsEditing,
} from "./ProjectConnectionRow";
import {
  AlertDialog,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from "@/shared/ui/alert-dialog";
import { Button } from "@/shared/ui/button";
import { Checkbox } from "@/shared/ui/checkbox";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/shared/ui/dialog";
import { Input } from "@/shared/ui/input";
import { Textarea } from "@/shared/ui/textarea";
import {
  PROJECT_DETAIL_PANEL_CLASS,
  PROJECT_DETAIL_PANEL_MESSAGE_CLASS,
  PROJECT_PANEL_ACTION_BUTTON_CLASS,
} from "./projectPanelStyles";
import { buildProjectConnectionSecretChanges } from "./projectConnectionSecrets";

type SecretRow = { id: string; key: string; value: string };
const MAX_NAME_BYTES = 128;
const MAX_PROVIDER_BYTES = 64;
const MAX_COMMAND_BYTES = 1024;
const MAX_ARGS = 128;
const MAX_ARG_BYTES = 4096;

function utf8ByteLength(value: string) {
  return new TextEncoder().encode(value).byteLength;
}

function emptySecretRow(): SecretRow {
  return { id: crypto.randomUUID(), key: "", value: "" };
}

function ConnectionDialog({
  communityName,
  connection,
  onOpenChange,
  onSave,
  onTest,
  open,
  pending,
  projectName,
  projectScope,
}: {
  communityName: string;
  connection: ProjectConnection | null;
  onOpenChange: (open: boolean) => void;
  onSave: (
    input: ProjectConnectionDraft & { id?: string },
  ) => Promise<ProjectConnection>;
  onTest: (connection: ProjectConnection) => Promise<void>;
  open: boolean;
  pending: boolean;
  projectName: string;
  projectScope: ProjectConnectionScope;
}) {
  const [name, setName] = React.useState("");
  const [provider, setProvider] = React.useState("");
  const [command, setCommand] = React.useState("");
  const [argsText, setArgsText] = React.useState("");
  const [showTechnicalDetails, setShowTechnicalDetails] = React.useState(false);
  const [secrets, setSecrets] = React.useState<SecretRow[]>([]);
  const [removedEnvKeys, setRemovedEnvKeys] = React.useState<string[]>([]);
  const [trusted, setTrusted] = React.useState(false);
  const [executionDirty, setExecutionDirty] = React.useState(false);
  const [error, setError] = React.useState<string | null>(null);
  const mounted = useMountedRef();

  React.useEffect(() => {
    if (!open) {
      setName("");
      setProvider("");
      setCommand("");
      setArgsText("");
      setSecrets([]);
      setRemovedEnvKeys([]);
      setTrusted(false);
      setExecutionDirty(false);
      setError(null);
      return;
    }
    setName(connection?.name ?? "");
    setProvider(connection?.provider ?? "");
    setCommand(connection?.command ?? "");
    setArgsText(connection?.args.join("\n") ?? "");
    setSecrets(
      connection?.envKeys.map((key) => ({
        id: crypto.randomUUID(),
        key,
        value: "",
      })) ?? [emptySecretRow()],
    );
    setRemovedEnvKeys([]);
    setShowTechnicalDetails(Boolean(connection));
    setTrusted(false);
    setExecutionDirty(!connection || projectConnectionNeedsEditing(connection));
    setError(null);
  }, [connection, open]);

  const requiresApproval = !connection || executionDirty;
  const parsedArgs = argsText
    .split("\n")
    .map((arg) => arg.trim())
    .filter(Boolean);
  const nameError =
    name.trim() && utf8ByteLength(name.trim()) > MAX_NAME_BYTES
      ? `Keep the connection name to ${MAX_NAME_BYTES} bytes or fewer.`
      : null;
  const providerError =
    provider.trim() && utf8ByteLength(provider.trim()) > MAX_PROVIDER_BYTES
      ? `Keep the service name to ${MAX_PROVIDER_BYTES} bytes or fewer.`
      : null;
  const commandError =
    command.trim() && utf8ByteLength(command.trim()) > MAX_COMMAND_BYTES
      ? `Keep the command to ${MAX_COMMAND_BYTES} bytes or fewer.`
      : null;
  const argsError =
    parsedArgs.length > MAX_ARGS ||
    parsedArgs.some((arg) => utf8ByteLength(arg) > MAX_ARG_BYTES)
      ? `Use no more than ${MAX_ARGS} arguments, with each ${MAX_ARG_BYTES} bytes or fewer.`
      : null;
  const hasValidFields = Boolean(
    name.trim() &&
      provider.trim() &&
      command.trim() &&
      !nameError &&
      !providerError &&
      !commandError &&
      !argsError,
  );
  const secretChanges = buildProjectConnectionSecretChanges(
    secrets,
    connection?.envKeys ?? [],
    removedEnvKeys,
  );
  const canSubmit =
    !pending &&
    hasValidFields &&
    secretChanges.ok &&
    (!requiresApproval || trusted);

  async function handleSubmit(event: React.FormEvent) {
    event.preventDefault();
    if (!canSubmit) return;
    setError(null);
    try {
      const saved = await onSave({
        ...(connection ? { id: connection.id } : {}),
        projectScope,
        name: name.trim(),
        provider: provider.trim(),
        command: command.trim(),
        args: parsedArgs,
        env: secretChanges.env,
        removeEnvKeys: secretChanges.removeEnvKeys,
        executionAcknowledged: requiresApproval ? trusted : false,
      });
      if (requiresApproval) {
        recordPendingProjectConnectionAutomaticTest(saved);
      }
      if (!mounted.current) return;
      onOpenChange(false);
      if (requiresApproval) {
        clearPendingProjectConnectionAutomaticTest(projectScope, saved.id);
        await onTest(saved);
      }
    } catch (cause) {
      if (!mounted.current) return;
      setError(
        cause instanceof Error
          ? cause.message
          : "Couldn't save this connection. Check the details and try again.",
      );
    } finally {
      markE2EProjectConnectionSettled("save");
    }
  }

  return (
    <Dialog
      onOpenChange={(nextOpen) => {
        if (!pending) onOpenChange(nextOpen);
      }}
      open={open}
    >
      <DialogContent className="max-w-xl" showCloseButton={!pending}>
        <form onSubmit={handleSubmit}>
          <DialogHeader>
            <DialogTitle>
              {connection
                ? `Edit ${connection.name} for ${projectName}`
                : `Add Project connection to ${projectName}`}
            </DialogTitle>
            <DialogDescription>
              {connection
                ? "Change how this connection runs and what it can access."
                : "Set up an MCP server and review its access before saving."}
            </DialogDescription>
          </DialogHeader>
          <fieldset className="mt-4 grid gap-x-4 gap-y-2 border-border/60 border-y py-3 text-xs sm:grid-cols-[auto_minmax(0,1fr)]">
            <legend className="sr-only">Connection scope</legend>
            <dl className="contents">
              <dt className="font-medium text-muted-foreground">Project</dt>
              <dd className="min-w-0 font-medium text-foreground">
                {projectName}
              </dd>
              <dt className="font-medium text-muted-foreground">Community</dt>
              <dd className="min-w-0 text-foreground">{communityName}</dd>
              <dt className="font-medium text-muted-foreground">Relay</dt>
              <dd className="min-w-0 break-all font-mono text-foreground">
                {projectScope.relayUrl}
              </dd>
              <dt className="font-medium text-muted-foreground">Applies to</dt>
              <dd className="min-w-0 text-foreground">
                Every repository in {projectName}
              </dd>
              <dt className="font-medium text-muted-foreground">Credentials</dt>
              <dd className="min-w-0 text-foreground">This device only</dd>
            </dl>
          </fieldset>
          <div className="space-y-4 py-5">
            <div className="space-y-1.5">
              <label
                className="text-sm font-medium text-foreground"
                htmlFor="connection-name"
              >
                Connection name <span aria-hidden="true">*</span>
              </label>
              <Input
                aria-describedby={
                  nameError ? "connection-name-error" : undefined
                }
                aria-invalid={Boolean(nameError) || undefined}
                disabled={pending}
                id="connection-name"
                maxLength={128}
                onChange={(event) => setName(event.target.value)}
                placeholder="Analytics"
                required
                value={name}
              />
              {nameError ? (
                <p
                  className="text-xs text-destructive"
                  id="connection-name-error"
                >
                  {nameError}
                </p>
              ) : null}
            </div>
            <div className="space-y-1.5">
              <label
                className="text-sm font-medium text-foreground"
                htmlFor="connection-provider"
              >
                Service <span aria-hidden="true">*</span>
              </label>
              <Input
                aria-describedby={
                  providerError ? "connection-provider-error" : undefined
                }
                aria-invalid={Boolean(providerError) || undefined}
                disabled={pending}
                id="connection-provider"
                maxLength={64}
                onChange={(event) => setProvider(event.target.value)}
                placeholder="Google Analytics"
                required
                value={provider}
              />
              {providerError ? (
                <p
                  className="text-xs text-destructive"
                  id="connection-provider-error"
                >
                  {providerError}
                </p>
              ) : null}
            </div>
            <div className="space-y-1.5">
              <label
                className="text-sm font-medium text-foreground"
                htmlFor="connection-command"
              >
                Connection command <span aria-hidden="true">*</span>
              </label>
              <Input
                autoCapitalize="off"
                autoCorrect="off"
                disabled={pending}
                aria-describedby={
                  commandError
                    ? "connection-command-help connection-command-error"
                    : "connection-command-help"
                }
                aria-invalid={Boolean(commandError) || undefined}
                id="connection-command"
                maxLength={1024}
                onChange={(event) => {
                  setCommand(event.target.value);
                  setExecutionDirty(true);
                  setTrusted(false);
                }}
                placeholder="/absolute/path/to/mcp-server"
                required
                spellCheck={false}
                value={command}
              />
              <p
                className="text-xs text-muted-foreground"
                id="connection-command-help"
              >
                Enter the executable's absolute path. Buzz runs it directly
                without a shell.
              </p>
              {commandError ? (
                <p
                  className="text-xs text-destructive"
                  id="connection-command-error"
                >
                  {commandError}
                </p>
              ) : null}
            </div>
            <button
              aria-expanded={showTechnicalDetails}
              className="inline-flex h-8 items-center gap-1.5 text-sm font-medium text-foreground"
              onClick={() => setShowTechnicalDetails((value) => !value)}
              type="button"
            >
              Technical details
              <ChevronDown
                className={`h-4 w-4 transition-transform ${showTechnicalDetails ? "rotate-180" : ""}`}
              />
            </button>
            {showTechnicalDetails ? (
              <div className="space-y-4 rounded-xl border border-border/60 bg-muted/15 p-4">
                <div className="space-y-1.5">
                  <label
                    className="text-sm font-medium text-foreground"
                    htmlFor="connection-args"
                  >
                    Arguments
                  </label>
                  <Textarea
                    aria-describedby={
                      argsError ? "connection-args-error" : undefined
                    }
                    aria-invalid={Boolean(argsError) || undefined}
                    className="min-h-24 font-mono text-xs"
                    disabled={pending}
                    id="connection-args"
                    onChange={(event) => {
                      setArgsText(event.target.value);
                      setExecutionDirty(true);
                      setTrusted(false);
                    }}
                    placeholder={"--account\n123456"}
                    value={argsText}
                  />
                  <p className="text-xs text-muted-foreground">
                    Enter one argument per non-empty line. Buzz trims
                    surrounding whitespace. Do not put secrets here.
                  </p>
                  {argsError ? (
                    <p
                      className="text-xs text-destructive"
                      id="connection-args-error"
                    >
                      {argsError}
                    </p>
                  ) : null}
                </div>
                <div className="space-y-2">
                  <div className="flex items-center justify-between gap-3">
                    <div>
                      <p className="text-sm font-medium text-foreground">
                        Secrets
                      </p>
                      <p className="text-xs text-muted-foreground">
                        Values are saved locally and never shown again.
                      </p>
                    </div>
                    <Button
                      disabled={pending}
                      onClick={() => {
                        setSecrets((rows) => [...rows, emptySecretRow()]);
                        setExecutionDirty(true);
                        setTrusted(false);
                      }}
                      size="xs"
                      type="button"
                      variant="outline"
                    >
                      <Plus className="h-3.5 w-3.5" />
                      Add secret
                    </Button>
                  </div>
                  {secrets.length > 0 ? (
                    <div
                      aria-hidden="true"
                      className="grid grid-cols-[minmax(0,1fr)_minmax(0,1.4fr)_auto] gap-2 px-0.5 text-2xs font-medium text-muted-foreground"
                    >
                      <span>Name</span>
                      <span>Value</span>
                      <span className="w-6" />
                    </div>
                  ) : null}
                  {secrets.map((row, index) => (
                    <div
                      className="grid grid-cols-[minmax(0,1fr)_minmax(0,1.4fr)_auto] gap-2"
                      key={row.id}
                    >
                      <Input
                        aria-label={`Secret ${index + 1} name`}
                        autoCapitalize="characters"
                        autoCorrect="off"
                        disabled={
                          pending ||
                          Boolean(connection?.envKeys.includes(row.key))
                        }
                        onChange={(event) => {
                          const key = event.target.value.toUpperCase();
                          setSecrets((rows) =>
                            rows.map((item, rowIndex) =>
                              rowIndex === index ? { ...item, key } : item,
                            ),
                          );
                          setExecutionDirty(true);
                          setTrusted(false);
                        }}
                        placeholder="API_TOKEN"
                        spellCheck={false}
                        value={row.key}
                      />
                      <Input
                        aria-label={`Secret ${index + 1} value`}
                        autoComplete="off"
                        disabled={pending}
                        onChange={(event) => {
                          const value = event.target.value;
                          setSecrets((rows) =>
                            rows.map((item, rowIndex) =>
                              rowIndex === index ? { ...item, value } : item,
                            ),
                          );
                          setExecutionDirty(true);
                          setTrusted(false);
                        }}
                        placeholder={
                          connection?.envKeys.includes(row.key)
                            ? "Leave blank to keep saved value"
                            : "Secret value"
                        }
                        type="password"
                        value={row.value}
                      />
                      <Button
                        aria-label={`Remove secret ${index + 1}`}
                        disabled={pending}
                        onClick={() => {
                          if (connection?.envKeys.includes(row.key)) {
                            setRemovedEnvKeys((keys) => [...keys, row.key]);
                          }
                          setSecrets((rows) =>
                            rows.filter((_, rowIndex) => rowIndex !== index),
                          );
                          setExecutionDirty(true);
                          setTrusted(false);
                        }}
                        size="icon-xs"
                        type="button"
                        variant="ghost"
                      >
                        <Trash2 className="h-4 w-4" />
                      </Button>
                    </div>
                  ))}
                  {!secretChanges.ok ? (
                    <p className="text-xs text-destructive" role="alert">
                      {secretChanges.error}
                    </p>
                  ) : null}
                </div>
              </div>
            ) : null}
            {requiresApproval ? (
              <label
                className="flex items-start gap-2 rounded-xl border border-amber-500/30 bg-amber-500/10 p-3 text-xs text-foreground"
                htmlFor="connection-trusted-command"
              >
                <Checkbox
                  checked={trusted}
                  disabled={pending}
                  id="connection-trusted-command"
                  onCheckedChange={(checked) => setTrusted(checked === true)}
                />
                <span>
                  I trust this executable and the arguments above to run without
                  a sandbox. It can access my files and network, plus these
                  secrets and anything their credentials allow.
                </span>
              </label>
            ) : null}
            {error ? (
              <p className="text-sm text-destructive" role="alert">
                {error}
              </p>
            ) : null}
          </div>
          <DialogFooter>
            <Button
              disabled={pending}
              onClick={() => onOpenChange(false)}
              type="button"
              variant="outline"
            >
              Cancel
            </Button>
            <Button disabled={!canSubmit} type="submit">
              {pending
                ? "Saving…"
                : requiresApproval
                  ? "Save and test"
                  : "Save changes"}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}

export function ProjectConnectionsPanel({
  projectName,
  projectScope,
}: {
  projectName: string;
  projectScope: ProjectConnectionScope;
}) {
  const { activeCommunity } = useCommunities();
  const communityName =
    activeCommunity?.relayUrl === projectScope.relayUrl
      ? activeCommunity.name
      : projectScope.relayUrl;
  const query = useProjectConnectionsQuery(projectScope);
  const createMutation = useCreateProjectConnectionMutation(projectScope);
  const updateMutation = useUpdateProjectConnectionMutation(projectScope);
  const testMutation = useTestProjectConnectionMutation(projectScope);
  const deleteMutation = useDeleteProjectConnectionMutation(projectScope);
  const [dialogOpen, setDialogOpen] = React.useState(false);
  const [editing, setEditing] = React.useState<ProjectConnection | null>(null);
  const [removing, setRemoving] = React.useState<ProjectConnection | null>(
    null,
  );
  const [removeError, setRemoveError] = React.useState<string | null>(null);
  const scopeKey = `${projectScope.relayUrl}\0${projectScope.operatorPubkey}\0${projectScope.projectAddress}`;
  const mounted = useMountedRef();

  function openAdd() {
    setEditing(null);
    setDialogOpen(true);
  }

  async function handleTest(connection: ProjectConnection) {
    clearPendingProjectConnectionAutomaticTest(projectScope, connection.id);
    try {
      const tested = await testMutation.mutateAsync(connection.id);
      if (!mounted.current) return;
      if (tested.health.status === "ready") {
        toast.success(`Tools found for ${connection.name}.`);
      } else {
        toast.error(
          tested.health.detail ?? `${connection.name} needs attention.`,
        );
      }
    } catch (cause) {
      if (!mounted.current) return;
      toast.error(
        cause instanceof Error
          ? `Couldn't test ${connection.name}: ${cause.message}`
          : `Couldn't test ${connection.name}. Check its details and try again.`,
      );
    } finally {
      markE2EProjectConnectionSettled("test");
    }
  }

  async function handleDelete() {
    if (!removing) return;
    const removingConnection = removing;
    setRemoveError(null);
    try {
      await deleteMutation.mutateAsync(removingConnection.id);
      if (!mounted.current) return;
      toast.success(`${removingConnection.name} removed.`);
      setRemoving(null);
    } catch (cause) {
      if (!mounted.current) return;
      setRemoveError(
        cause instanceof Error
          ? `Couldn't remove ${removingConnection.name}: ${cause.message}`
          : `Couldn't remove ${removingConnection.name}. Nothing was changed.`,
      );
    } finally {
      markE2EProjectConnectionSettled("delete");
    }
  }

  const connections = query.data ?? [];
  const pendingAutomaticTestIds =
    pendingProjectConnectionAutomaticTestIds(projectScope);

  React.useEffect(() => {
    if (!query.data || query.isFetching) return;
    const recordedConnectionIds =
      pendingProjectConnectionAutomaticTestIds(projectScope);
    const connectionsById = new Map(
      query.data.map((connection) => [connection.id, connection]),
    );
    for (const connectionId of recordedConnectionIds) {
      const connection = connectionsById.get(connectionId);
      if (connection?.health.status !== "not_tested") {
        clearPendingProjectConnectionAutomaticTest(projectScope, connectionId);
      }
    }
  }, [projectScope, query.data, query.isFetching]);

  return (
    <>
      <div
        className={PROJECT_DETAIL_PANEL_CLASS}
        data-project-detail-panel
        data-testid="project-connections-panel"
      >
        <div className="flex min-h-14 flex-wrap items-center gap-3 border-border/50 border-b px-4 py-3">
          <div className="flex min-w-[min(100%,18rem)] flex-1 items-start gap-3">
            <Link2 className="mt-0.5 h-4 w-4 shrink-0 text-muted-foreground" />
            <div className="min-w-0 flex-1">
              <h3 className="text-sm font-medium text-foreground">
                Connections
              </h3>
              <p className="text-xs text-muted-foreground">
                Make tools available across this Project. Credentials stay on
                this device.
              </p>
            </div>
          </div>
          <Button
            className={`${PROJECT_PANEL_ACTION_BUTTON_CLASS} ml-auto`}
            onClick={openAdd}
            size="sm"
            title="Add connection"
          >
            <Plus className="h-4 w-4" />
            Add connection
          </Button>
        </div>

        {query.isPending ? (
          <div className={PROJECT_DETAIL_PANEL_MESSAGE_CLASS} role="status">
            <LoaderCircle className="mx-auto mb-2 h-5 w-5 animate-spin" />
            Loading connections…
          </div>
        ) : query.isError ? (
          <div className={PROJECT_DETAIL_PANEL_MESSAGE_CLASS}>
            <XCircle className="mx-auto mb-2 h-5 w-5 text-destructive" />
            <p>
              Couldn't load connections. Your saved connections were not
              changed.
            </p>
            <Button
              className="mt-3"
              onClick={() => void query.refetch()}
              size="sm"
              variant="outline"
            >
              Try again
            </Button>
          </div>
        ) : connections.length === 0 ? (
          <div className={PROJECT_DETAIL_PANEL_MESSAGE_CLASS}>
            <Link2 className="mx-auto mb-2 h-5 w-5 text-muted-foreground" />
            <p className="font-medium text-foreground">No connections yet</p>
            <p className="mt-1 text-sm text-muted-foreground">
              Add an MCP server, inspect its tools, and keep its credentials
              local to this device and associated with this Project.
            </p>
            <Button className="mt-3" onClick={openAdd} size="sm">
              Add connection
            </Button>
          </div>
        ) : (
          <div className="divide-y divide-border/50">
            {connections.map((connection) => (
              <ProjectConnectionRow
                automaticTestInterrupted={
                  connection.health.status === "not_tested" &&
                  pendingAutomaticTestIds.has(connection.id) &&
                  !(
                    testMutation.isPending &&
                    testMutation.variables === connection.id
                  )
                }
                connection={connection}
                key={connection.id}
                onEdit={() => {
                  setEditing(connection);
                  setDialogOpen(true);
                }}
                onRemove={() => {
                  setRemoveError(null);
                  setRemoving(connection);
                }}
                onTest={() => void handleTest(connection)}
                testPending={testMutation.isPending}
                testing={
                  testMutation.isPending &&
                  testMutation.variables === connection.id
                }
              />
            ))}
          </div>
        )}
      </div>

      <ConnectionDialog
        communityName={communityName}
        connection={editing}
        key={scopeKey}
        onOpenChange={setDialogOpen}
        onSave={(input) =>
          input.id
            ? updateMutation.mutateAsync({
                ...input,
                id: input.id,
              })
            : createMutation.mutateAsync(input)
        }
        onTest={handleTest}
        open={dialogOpen}
        pending={createMutation.isPending || updateMutation.isPending}
        projectName={projectName}
        projectScope={projectScope}
      />

      <AlertDialog
        onOpenChange={(open) => {
          if (!open && !deleteMutation.isPending) {
            setRemoving(null);
            setRemoveError(null);
          }
        }}
        open={Boolean(removing)}
      >
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>
              {`Remove ${removing?.name ?? "connection"}?`}
            </AlertDialogTitle>
            <AlertDialogDescription>
              Buzz will remove the saved connection and its credentials from
              this device. This does not delete data from the connected service.
            </AlertDialogDescription>
          </AlertDialogHeader>
          {removeError ? (
            <p className="text-sm text-destructive" role="alert">
              {removeError}
            </p>
          ) : null}
          <AlertDialogFooter>
            <AlertDialogCancel disabled={deleteMutation.isPending}>
              Cancel
            </AlertDialogCancel>
            <Button
              disabled={deleteMutation.isPending}
              onClick={() => void handleDelete()}
              variant="destructive"
            >
              {deleteMutation.isPending ? "Removing…" : "Remove connection"}
            </Button>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </>
  );
}
