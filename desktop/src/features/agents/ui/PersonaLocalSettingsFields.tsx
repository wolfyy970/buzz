import { ChevronDown } from "lucide-react";
import { AnimatePresence, motion } from "motion/react";
import type { ComponentProps } from "react";

import { cn } from "@/shared/lib/cn";
import { PersonaAdvancedFields } from "./PersonaAdvancedFields";

type PersonaLocalSettingsFieldsProps = ComponentProps<
  typeof PersonaAdvancedFields
> & {
  missingEnvKeys: string[];
  onOpenChange: (open: boolean) => void;
  open: boolean;
  transition: ComponentProps<typeof motion.div>["transition"];
};

export function PersonaLocalSettingsFields({
  missingEnvKeys,
  onOpenChange,
  open,
  requiredEnvKeys = [],
  transition,
  ...advancedFields
}: PersonaLocalSettingsFieldsProps) {
  const hasMissingRequiredEnvironment = missingEnvKeys.some((key) =>
    requiredEnvKeys.includes(key),
  );

  return (
    <section
      className="scroll-mt-4 space-y-3"
      id="persona-local-settings-section"
      tabIndex={-1}
    >
      <div>
        <h3>
          <button
            aria-expanded={open}
            className="flex w-full items-center justify-between gap-3 rounded-lg py-1 text-left text-base font-semibold text-foreground transition-colors hover:text-foreground/80 focus-visible:outline-hidden focus-visible:ring-2 focus-visible:ring-ring"
            onClick={() => onOpenChange(!open)}
            type="button"
          >
            <span>Local settings</span>
            <span className="flex items-center gap-1.5">
              {hasMissingRequiredEnvironment ? (
                <span
                  aria-hidden="true"
                  className="rounded-full bg-destructive/10 px-2 py-0.5 text-xs text-destructive"
                  data-testid="persona-advanced-required-badge"
                >
                  Required
                </span>
              ) : null}
              <ChevronDown
                className={cn(
                  "h-4 w-4 text-muted-foreground transition-transform duration-150 ease-out",
                  open && "rotate-180",
                )}
              />
            </span>
          </button>
        </h3>
        <p className="mt-1 text-xs text-muted-foreground">
          Instance names, environment, and model tuning
        </p>
      </div>
      <AnimatePresence initial={false}>
        {open ? (
          <motion.div
            animate={{ height: "auto", opacity: 1, scale: 1 }}
            className="origin-top overflow-hidden"
            exit={{ height: 0, opacity: 0, scale: 0.98 }}
            initial={{ height: 0, opacity: 0, scale: 0.98 }}
            key="persona-advanced-fields"
            transition={transition}
          >
            <PersonaAdvancedFields
              {...advancedFields}
              requiredEnvKeys={requiredEnvKeys}
            />
          </motion.div>
        ) : null}
      </AnimatePresence>
    </section>
  );
}
