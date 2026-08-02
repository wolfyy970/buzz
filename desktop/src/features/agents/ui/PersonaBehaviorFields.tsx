import { Input } from "@/shared/ui/input";
import { cn } from "@/shared/lib/cn";
import {
  PERSONA_FIELD_CONTROL_CLASS,
  PERSONA_FIELD_SHELL_CLASS,
  PERSONA_LABEL_OPTIONAL_CLASS,
} from "./agentConfigOptions";
import type { PersonaBehaviorDraft } from "./personaBehaviorDraft";
import { CreateAgentRespondToField } from "./RespondToField";

export function PersonaBehaviorFields({
  disabled,
  draft,
  onChange,
}: {
  disabled: boolean;
  draft: PersonaBehaviorDraft;
  onChange: (value: PersonaBehaviorDraft) => void;
}) {
  return (
    <section
      className="scroll-mt-4 space-y-4"
      id="persona-behavior-section"
      tabIndex={-1}
    >
      <div>
        <h3 className="text-base font-semibold text-foreground">Behavior</h3>
        <p className="mt-1 text-xs text-muted-foreground">
          Choose who can use this agent and how much work it can handle at once.
        </p>
      </div>

      <CreateAgentRespondToField
        allowlist={draft.respondToAllowlist}
        disabled={disabled}
        mode={draft.respondTo ?? "owner-only"}
        onAllowlistChange={(allowlist) =>
          onChange({
            ...draft,
            respondToAllowlist: allowlist,
          })
        }
        onModeChange={(mode) => onChange({ ...draft, respondTo: mode })}
        variant="persona"
      />

      <div className="max-w-sm space-y-1.5">
        <label
          className="text-sm font-medium text-foreground"
          htmlFor="persona-parallelism"
        >
          Parallel conversations
          <span className={PERSONA_LABEL_OPTIONAL_CLASS}>Optional</span>
        </label>
        <div
          className={cn(
            "flex min-h-11 items-center px-3",
            PERSONA_FIELD_SHELL_CLASS,
          )}
        >
          <Input
            aria-describedby="persona-parallelism-help"
            className={cn(
              "h-8 px-0 py-0 leading-6",
              PERSONA_FIELD_CONTROL_CLASS,
            )}
            disabled={disabled}
            id="persona-parallelism"
            inputMode="numeric"
            max={32}
            min={1}
            onChange={(event) =>
              onChange({
                ...draft,
                parallelism: event.target.value,
              })
            }
            placeholder="1"
            type="number"
            value={draft.parallelism}
          />
        </div>
        <p
          className="text-xs text-muted-foreground"
          id="persona-parallelism-help"
        >
          Conversations each running instance can handle at once (1–32).
        </p>
      </div>
    </section>
  );
}
