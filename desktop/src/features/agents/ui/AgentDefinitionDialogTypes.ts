import type { ReactNode } from "react";

import type {
  AcpRuntimeCatalogEntry,
  AgentToolRequirement,
  CreatePersonaInput,
  ManagedAgent,
  UpdatePersonaInput,
} from "@/shared/api/types";

export type AgentDefinitionSubmitOptions = {
  publishCatalogUpdates: boolean;
};

export type AgentDefinitionDialogProps = {
  open: boolean;
  title: string;
  description: string;
  submitLabel: string;
  initialValues: CreatePersonaInput | UpdatePersonaInput | null;
  error: Error | null;
  isPending: boolean;
  runtimes: AcpRuntimeCatalogEntry[];
  runtimesLoading?: boolean;
  onOpenChange: (open: boolean) => void;
  onSubmit: (
    input: CreatePersonaInput | UpdatePersonaInput,
    options: AgentDefinitionSubmitOptions,
  ) => Promise<unknown>;
  /** Publishes saved changes when the edited agent is shared in the catalog. */
  publishCatalogUpdatesOnSave?: boolean;
  /** Managed instances currently linked to the edited template. */
  affectedAgents?: ManagedAgent[];
  /** Rendered below the form fields in create mode only. */
  createRunSection?:
    | ReactNode
    | ((toolRequirements: AgentToolRequirement[]) => ReactNode);
  /** Extra create-mode submit gate (e.g. incomplete provider config). */
  createSubmitBlocked?:
    | boolean
    | ((toolRequirements: AgentToolRequirement[]) => boolean);
  /** User-facing recovery for the extra create-mode submit gate. */
  createSubmitBlockReason?:
    | string
    | null
    | ((toolRequirements: AgentToolRequirement[]) => string | null);
};
