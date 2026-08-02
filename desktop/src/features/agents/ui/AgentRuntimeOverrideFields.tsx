import { cn } from "@/shared/lib/cn";
import { Badge } from "@/shared/ui/badge";
import { Button } from "@/shared/ui/button";
import { Input } from "@/shared/ui/input";
import {
  PERSONA_FIELD_CONTROL_CLASS,
  PERSONA_FIELD_SHELL_CLASS,
  PERSONA_LABEL_OPTIONAL_CLASS,
  type PersonaDropdownOption,
} from "./agentConfigOptions";
import { PersonaDropdownField } from "./PersonaDropdownField";

type OverrideHeaderProps = {
  changedForAgent: boolean;
  disabled: boolean;
  id: string;
  label: string;
  onReset: () => void;
  required: boolean;
  resetTestId: string;
};

function OverrideHeader({
  changedForAgent,
  disabled,
  id,
  label,
  onReset,
  required,
  resetTestId,
}: OverrideHeaderProps) {
  return (
    <div className="flex items-center justify-between gap-3">
      <div className="flex items-center gap-2">
        <label className="text-sm font-medium text-foreground" htmlFor={id}>
          {label}
          {required ? (
            <span className="ml-1 text-destructive" aria-hidden="true">
              *
            </span>
          ) : (
            <span className={PERSONA_LABEL_OPTIONAL_CLASS}>Optional</span>
          )}
        </label>
        {changedForAgent ? (
          <Badge variant="secondary">Changed for this agent</Badge>
        ) : null}
      </div>
      {changedForAgent ? (
        <Button
          data-testid={resetTestId}
          disabled={disabled}
          onClick={onReset}
          size="xs"
          type="button"
          variant="ghost"
        >
          Reset to template
        </Button>
      ) : null}
    </div>
  );
}

type AgentProviderFieldProps = {
  changedForAgent: boolean;
  customValue: string;
  disabled: boolean;
  isCustomEditing: boolean;
  onCustomChange: (value: string) => void;
  onReset: () => void;
  onValueChange: (value: string) => void;
  options: PersonaDropdownOption[];
  required: boolean;
  selectedValue: string;
};

export function AgentProviderField({
  changedForAgent,
  customValue,
  disabled,
  isCustomEditing,
  onCustomChange,
  onReset,
  onValueChange,
  options,
  required,
  selectedValue,
}: AgentProviderFieldProps) {
  return (
    <div className="space-y-1.5">
      <OverrideHeader
        changedForAgent={changedForAgent}
        disabled={disabled}
        id="edit-agent-llm-provider"
        label="LLM provider"
        onReset={onReset}
        required={required}
        resetTestId="reset-agent-provider-to-template"
      />
      <PersonaDropdownField
        disabled={disabled}
        id="edit-agent-llm-provider"
        onValueChange={onValueChange}
        options={options}
        placeholder="Default (auto)"
        value={selectedValue}
      />
      {isCustomEditing ? (
        <div
          className={cn(
            "mt-2 flex min-h-11 items-center px-3",
            PERSONA_FIELD_SHELL_CLASS,
          )}
        >
          <Input
            aria-label="Custom provider ID"
            autoCorrect="off"
            className={cn(
              "h-8 px-0 py-0 leading-6",
              PERSONA_FIELD_CONTROL_CLASS,
            )}
            disabled={disabled}
            id="edit-agent-custom-provider"
            onChange={(event) => onCustomChange(event.target.value)}
            placeholder="Custom provider ID"
            value={customValue}
          />
        </div>
      ) : null}
    </div>
  );
}

type AgentModelFieldProps = {
  changedForAgent: boolean;
  customValue: string;
  disabled: boolean;
  discoveryLoading: boolean;
  onCustomChange: (value: string) => void;
  onReset: () => void;
  onValueChange: (value: string) => void;
  options: PersonaDropdownOption[];
  required: boolean;
  showCustomInput: boolean;
  statusMessage: string | null;
  selectedValue: string;
};

export function AgentModelField({
  changedForAgent,
  customValue,
  disabled,
  discoveryLoading,
  onCustomChange,
  onReset,
  onValueChange,
  options,
  required,
  showCustomInput,
  statusMessage,
  selectedValue,
}: AgentModelFieldProps) {
  return (
    <div className="space-y-1.5">
      <OverrideHeader
        changedForAgent={changedForAgent}
        disabled={disabled}
        id="edit-agent-model"
        label="Model"
        onReset={onReset}
        required={required}
        resetTestId="reset-agent-model-to-template"
      />
      <PersonaDropdownField
        disabled={disabled || discoveryLoading}
        id="edit-agent-model"
        onValueChange={onValueChange}
        options={options}
        placeholder="Default model"
        value={selectedValue}
      />
      {showCustomInput ? (
        <div
          className={cn(
            "mt-2 flex min-h-11 items-center px-3",
            PERSONA_FIELD_SHELL_CLASS,
          )}
        >
          <Input
            aria-label="Custom model ID"
            autoCorrect="off"
            className={cn(
              "h-8 px-0 py-0 leading-6",
              PERSONA_FIELD_CONTROL_CLASS,
            )}
            disabled={disabled}
            id="edit-agent-custom-model"
            onChange={(event) => onCustomChange(event.target.value)}
            placeholder="Custom model ID"
            value={customValue}
          />
        </div>
      ) : null}
      {statusMessage ? (
        <p className="text-xs text-muted-foreground">{statusMessage}</p>
      ) : null}
    </div>
  );
}
