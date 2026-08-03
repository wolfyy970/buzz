import { cn } from "@/shared/lib/cn";
import { Textarea } from "@/shared/ui/textarea";
import {
  PERSONA_FIELD_CONTROL_CLASS,
  PERSONA_FIELD_SHELL_CLASS,
} from "./agentConfigOptions";

export function PersonaInstructionsFields({
  disabled,
  onChange,
  value,
}: {
  disabled: boolean;
  onChange: (value: string) => void;
  value: string;
}) {
  return (
    <section
      className="scroll-mt-4 space-y-1.5"
      id="persona-instructions-section"
      tabIndex={-1}
    >
      <h3 className="text-base font-semibold text-foreground">Instructions</h3>
      <label
        className="text-sm font-medium text-foreground"
        htmlFor="persona-system-prompt"
      >
        Agent instructions
      </label>
      <div className={PERSONA_FIELD_SHELL_CLASS}>
        <Textarea
          className={cn(
            "min-h-40 resize-y px-3 py-3 leading-5 [@media(max-height:600px)]:min-h-28",
            PERSONA_FIELD_CONTROL_CLASS,
          )}
          disabled={disabled}
          id="persona-system-prompt"
          onChange={(event) => onChange(event.target.value)}
          placeholder="Describe what this agent should do."
          value={value}
        />
      </div>
    </section>
  );
}
