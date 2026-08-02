import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from "@/shared/ui/alert-dialog";
import { Button } from "@/shared/ui/button";

export function PersonaDiscardConfirmation({
  isCreateMode,
  onDiscard,
  onOpenChange,
  open,
  templateName,
}: {
  isCreateMode: boolean;
  onDiscard: () => void;
  onOpenChange: (open: boolean) => void;
  open: boolean;
  templateName: string;
}) {
  return (
    <AlertDialog onOpenChange={onOpenChange} open={open}>
      <AlertDialogContent data-testid="persona-discard-confirmation">
        <AlertDialogHeader>
          <AlertDialogTitle>
            {isCreateMode
              ? "Discard this agent?"
              : `Discard changes to ${templateName.trim() || "this template"}?`}
          </AlertDialogTitle>
          <AlertDialogDescription>
            Your unsaved changes will be lost.
          </AlertDialogDescription>
        </AlertDialogHeader>
        <AlertDialogFooter>
          <AlertDialogCancel asChild>
            <Button type="button" variant="outline">
              Keep editing
            </Button>
          </AlertDialogCancel>
          <AlertDialogAction asChild>
            <Button onClick={onDiscard} type="button" variant="destructive">
              Discard changes
            </Button>
          </AlertDialogAction>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  );
}
