import type { ReactNode } from "react";

import { cn } from "@/shared/lib/cn";
import { Input } from "@/shared/ui/input";
import {
  PERSONA_FIELD_CONTROL_CLASS,
  PERSONA_FIELD_SHELL_CLASS,
} from "./agentConfigOptions";

export function PersonaIdentityFields({
  avatarEditor,
  disabled,
  displayName,
  isCreateMode,
  onDisplayNameChange,
}: {
  avatarEditor: ReactNode;
  disabled: boolean;
  displayName: string;
  isCreateMode: boolean;
  onDisplayNameChange: (value: string) => void;
}) {
  return (
    <section
      className="scroll-mt-4 space-y-1.5"
      id="persona-identity-section"
      tabIndex={-1}
    >
      <h3 className="text-base font-semibold text-foreground">Identity</h3>
      <div className={cn(!isCreateMode && "flex items-end gap-4")}>
        {!isCreateMode ? avatarEditor : null}
        <div className="min-w-0 flex-1 space-y-1.5">
          <label
            className="text-sm font-medium text-foreground"
            htmlFor="persona-display-name"
          >
            {isCreateMode ? "Agent name" : "Template name"}
          </label>
          <div
            className={cn(
              "flex min-h-11 items-center px-3",
              PERSONA_FIELD_SHELL_CLASS,
            )}
          >
            <Input
              autoCorrect="off"
              className={cn(
                "h-8 px-0 py-0 leading-6",
                PERSONA_FIELD_CONTROL_CLASS,
              )}
              disabled={disabled}
              id="persona-display-name"
              onChange={(event) => onDisplayNameChange(event.target.value)}
              placeholder="Fizz"
              value={displayName}
            />
          </div>
        </div>
      </div>
    </section>
  );
}
