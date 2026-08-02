import { toast } from "sonner";

import type {
  AgentPersona,
  CreateManagedAgentResponse,
  CreatePersonaInput,
  UpdatePersonaInput,
} from "@/shared/api/types";

type SubmitProfilePersonaDialogOptions = {
  createManagedAgentForPersona: (
    persona: AgentPersona,
  ) => Promise<CreateManagedAgentResponse>;
  createPersona: (input: CreatePersonaInput) => Promise<AgentPersona>;
  input: CreatePersonaInput | UpdatePersonaInput;
  onDone: () => void;
  showUpdateSuccess?: boolean;
  updatePersona: (input: UpdatePersonaInput) => Promise<AgentPersona>;
};

export async function submitProfilePersonaDialog({
  createManagedAgentForPersona,
  createPersona,
  input,
  onDone,
  showUpdateSuccess = true,
  updatePersona,
}: SubmitProfilePersonaDialogOptions) {
  try {
    let savedPersona: AgentPersona;
    if ("id" in input) {
      // Saving a template and applying it to an instance are separate user
      // decisions. Keep every linked instance pinned to its current revision
      // until the affected-agent review explicitly applies the new snapshot.
      savedPersona = await updatePersona(input);
      if (showUpdateSuccess) {
        toast.success(`Updated ${input.displayName}.`);
      }
    } else {
      const persona = await createPersona(input);
      savedPersona = persona;
      try {
        const created = await createManagedAgentForPersona(persona);
        if (created.spawnError) {
          toast.error(
            `${persona.displayName} was created, but it did not start: ${created.spawnError}`,
          );
        } else {
          toast.success(`Created and started ${created.agent.name}.`);
        }
        if (created.profileSyncError) {
          toast.warning(
            `${created.agent.name} was created, but profile sync failed: ${created.profileSyncError}`,
          );
        }
      } catch (error) {
        toast.error(
          error instanceof Error
            ? `${persona.displayName} was created, but the agent instance could not be created: ${error.message}`
            : `${persona.displayName} was created, but the agent instance could not be created.`,
        );
      }
    }

    onDone();
    return savedPersona;
  } catch (error) {
    toast.error(
      error instanceof Error ? error.message : "Failed to save agent.",
    );
    return null;
  }
}
