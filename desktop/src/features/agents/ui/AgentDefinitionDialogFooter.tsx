import { Button } from "@/shared/ui/button";

type AgentDefinitionDialogFooterProps = {
  canSubmit: boolean;
  errorMessage: string | null;
  isAvatarUploadPending: boolean;
  isPending: boolean;
  isTemplateEdit: boolean;
  lastAction: "save" | "publish" | null;
  onCancel: () => void;
  onSaveTemplate: () => void;
  pendingAction: "save" | "publish" | null;
  publishesCatalogUpdates: boolean;
  submitBlockReason: string | null;
  submitLabel: string;
};

export function AgentDefinitionDialogFooter({
  canSubmit,
  errorMessage,
  isAvatarUploadPending,
  isPending,
  isTemplateEdit,
  lastAction,
  onCancel,
  onSaveTemplate,
  pendingAction,
  publishesCatalogUpdates,
  submitBlockReason,
  submitLabel,
}: AgentDefinitionDialogFooterProps) {
  return (
    <div className="flex w-full flex-col gap-2">
      {errorMessage ? (
        <div
          aria-live="assertive"
          className="rounded-lg border border-destructive/30 bg-destructive/5 px-3 py-2"
          data-testid="persona-dialog-error"
          role="alert"
        >
          <p className="text-sm font-medium text-destructive">
            {lastAction === "publish"
              ? "Version wasn’t published"
              : "Changes weren’t saved"}
          </p>
          <p className="mt-0.5 text-xs text-destructive">{errorMessage}</p>
          {lastAction === "publish" ? (
            <p className="mt-1 text-xs text-muted-foreground">
              Your draft is saved. Fix the problem and try again.
            </p>
          ) : null}
        </div>
      ) : null}

      <div className="flex flex-wrap items-center justify-end gap-3">
        <div className="flex min-h-9 min-w-0 flex-1 flex-wrap items-center gap-3">
          {isTemplateEdit ? (
            <p className="max-w-md text-xs text-muted-foreground [@media(max-height:600px)]:hidden">
              Save keeps a draft. Publish creates a version, then lets you
              choose which agents use it.
            </p>
          ) : null}
          {submitBlockReason ? (
            <p
              aria-live="polite"
              className="text-2xs text-muted-foreground"
              data-testid="persona-dialog-submit-reason"
              role="status"
            >
              {submitBlockReason}
            </p>
          ) : null}
          {publishesCatalogUpdates ? (
            <p
              className="max-w-sm text-xs text-muted-foreground"
              data-testid="persona-dialog-catalog-publish-notice"
            >
              This agent is in the community catalog. Your changes will be
              published when you save.
            </p>
          ) : null}
        </div>

        <div
          className="flex shrink-0 items-center gap-2"
          data-testid="persona-dialog-actions"
        >
          <Button
            disabled={isPending || isAvatarUploadPending}
            onClick={onCancel}
            type="button"
            variant="outline"
          >
            Cancel
          </Button>
          {isTemplateEdit ? (
            <Button
              data-testid="persona-dialog-save-template"
              disabled={!canSubmit}
              onClick={onSaveTemplate}
              type="button"
              variant="outline"
            >
              {isPending && pendingAction === "save"
                ? "Saving..."
                : "Save draft"}
            </Button>
          ) : null}
          <Button
            data-testid="persona-dialog-submit"
            disabled={!canSubmit}
            form="persona-dialog-form"
            type="submit"
          >
            {isPending
              ? pendingAction === "publish"
                ? "Publishing..."
                : "Saving..."
              : isAvatarUploadPending
                ? "Uploading..."
                : isTemplateEdit
                  ? errorMessage && lastAction === "publish"
                    ? "Retry publishing"
                    : "Publish new version…"
                  : publishesCatalogUpdates
                    ? "Save and publish"
                    : submitLabel}
          </Button>
        </div>
      </div>
    </div>
  );
}
