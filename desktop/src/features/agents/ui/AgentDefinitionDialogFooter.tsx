import { Button } from "@/shared/ui/button";

type AgentDefinitionDialogFooterProps = {
  canSubmit: boolean;
  isAvatarUploadPending: boolean;
  isPending: boolean;
  isTemplateEdit: boolean;
  onCancel: () => void;
  onSaveTemplate: () => void;
  pendingAction: "save" | "publish" | null;
  publishesCatalogUpdates: boolean;
  submitBlockReason: string | null;
  submitLabel: string;
};

export function AgentDefinitionDialogFooter({
  canSubmit,
  isAvatarUploadPending,
  isPending,
  isTemplateEdit,
  onCancel,
  onSaveTemplate,
  pendingAction,
  publishesCatalogUpdates,
  submitBlockReason,
  submitLabel,
}: AgentDefinitionDialogFooterProps) {
  return (
    <div className="flex w-full flex-wrap items-center justify-end gap-3">
      <div className="flex min-h-9 min-w-0 flex-wrap items-center gap-3">
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

      <div className="flex items-center gap-2">
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
              : "Save changes"}
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
                ? "Publish version"
                : publishesCatalogUpdates
                  ? "Save and publish"
                  : submitLabel}
        </Button>
      </div>
    </div>
  );
}
