import {
  Bot,
  Cpu,
  GraduationCap,
  KeyRound,
  MessageSquareText,
  SlidersHorizontal,
  Wrench,
} from "lucide-react";
import type { ComponentType } from "react";

import { cn } from "@/shared/lib/cn";

type TemplateSection = {
  detail: string;
  icon: ComponentType<{ className?: string }>;
  id: string;
  label: string;
};

export function AgentTemplateContentsNav({
  model,
  onNavigate,
  runtime,
  skillCount,
  toolCount,
}: {
  model: string;
  onNavigate: (id: string) => void;
  runtime: string;
  skillCount: number;
  toolCount: number;
}) {
  const sections: TemplateSection[] = [
    {
      detail: "Name and avatar",
      icon: Bot,
      id: "persona-identity-section",
      label: "Identity",
    },
    {
      detail: "Role and limits",
      icon: MessageSquareText,
      id: "persona-instructions-section",
      label: "Instructions",
    },
    {
      detail: `${runtime} · ${model}`,
      icon: Cpu,
      id: "persona-model-section",
      label: "AI configuration",
    },
    {
      detail: `${skillCount} ${skillCount === 1 ? "Skill" : "Skills"}`,
      icon: GraduationCap,
      id: "persona-skills-section",
      label: "Skills",
    },
    {
      detail: `${toolCount} ${toolCount === 1 ? "requirement" : "requirements"}`,
      icon: Wrench,
      id: "persona-tools-section",
      label: "Tools",
    },
    {
      detail: "Access and parallel work",
      icon: SlidersHorizontal,
      id: "persona-behavior-section",
      label: "Behavior",
    },
  ];

  return (
    <section
      className="space-y-3 rounded-xl border border-border bg-muted/20 p-4"
      data-testid="template-contents-nav"
    >
      <div>
        <h3 className="text-base font-semibold text-foreground">
          Template setup
        </h3>
        <p className="mt-1 text-xs text-muted-foreground">
          Changes to these sections are versioned together.
        </p>
      </div>
      <div className="grid gap-2 sm:grid-cols-2 lg:grid-cols-3">
        {sections.map((section) => {
          const Icon = section.icon;
          return (
            <button
              className={cn(
                "flex min-w-0 items-center gap-2.5 rounded-lg border border-border bg-background px-3 py-2.5 text-left",
                "transition-colors hover:bg-muted/50 focus-visible:outline-hidden focus-visible:ring-2 focus-visible:ring-ring",
              )}
              data-testid={`template-section-${section.id}`}
              key={section.id}
              onClick={() => onNavigate(section.id)}
              type="button"
            >
              <Icon className="size-4 shrink-0 text-muted-foreground" />
              <span className="min-w-0">
                <span className="block text-xs font-medium text-foreground">
                  {section.label}
                </span>
                <span className="block truncate text-2xs text-muted-foreground">
                  {section.detail}
                </span>
              </span>
            </button>
          );
        })}
      </div>
      <button
        className={cn(
          "flex w-full items-center gap-2.5 rounded-lg px-1 py-1 text-left",
          "text-muted-foreground transition-colors hover:text-foreground focus-visible:outline-hidden focus-visible:ring-2 focus-visible:ring-ring",
        )}
        data-testid="template-section-persona-environment-section"
        onClick={() => onNavigate("persona-environment-section")}
        type="button"
      >
        <KeyRound className="size-4 shrink-0" />
        <span className="text-xs">
          <span className="font-medium text-foreground">Environment</span>
          <span className="ml-1">
            stays on this device and is not included in a version.
          </span>
        </span>
      </button>
    </section>
  );
}
