import { FileText, GraduationCap, Plus, Trash2 } from "lucide-react";
import * as React from "react";

import {
  type AgentSkill,
  type AgentSkillFile,
  synchronizeSkillMarkdown,
} from "@/shared/api/agentSkillTypes";
import { Button } from "@/shared/ui/button";
import { Input } from "@/shared/ui/input";
import { Textarea } from "@/shared/ui/textarea";

function emptySkill(): AgentSkill {
  return {
    name: "",
    description: "",
    files: [
      {
        path: "SKILL.md",
        content: synchronizeSkillMarkdown("", "", ""),
      },
    ],
  };
}

export function AgentSkillsSection({
  beforeAddAction,
  description = "Add reusable instructions and supporting files that travel with this template.",
  disabled,
  emptyMessage = "This template does not include any Skills.",
  status,
  onChange,
  value,
}: {
  beforeAddAction?: React.ReactNode;
  description?: string;
  disabled: boolean;
  emptyMessage?: string;
  status?: React.ReactNode;
  onChange: (value: AgentSkill[]) => void;
  value: AgentSkill[];
}) {
  const skillKeys = React.useRef<string[]>([]);
  const fileKeys = React.useRef<string[][]>([]);
  while (skillKeys.current.length < value.length) {
    skillKeys.current.push(crypto.randomUUID());
    fileKeys.current.push([]);
  }
  skillKeys.current.length = value.length;
  fileKeys.current.length = value.length;
  for (const [skillIndex, skill] of value.entries()) {
    const keys = fileKeys.current[skillIndex] ?? [];
    while (keys.length < skill.files.length) keys.push(crypto.randomUUID());
    keys.length = skill.files.length;
    fileKeys.current[skillIndex] = keys;
  }

  function updateSkill(index: number, patch: Partial<AgentSkill>) {
    onChange(
      value.map((skill, candidateIndex) => {
        if (candidateIndex !== index) return skill;
        const next = { ...skill, ...patch };
        if (patch.name === undefined && patch.description === undefined) {
          return next;
        }
        return {
          ...next,
          files: next.files.map((file) =>
            file.path === "SKILL.md"
              ? {
                  ...file,
                  content: synchronizeSkillMarkdown(
                    file.content,
                    next.name,
                    next.description,
                  ),
                }
              : file,
          ),
        };
      }),
    );
  }

  function updateFile(
    skillIndex: number,
    fileIndex: number,
    patch: Partial<AgentSkillFile>,
  ) {
    const skill = value[skillIndex];
    if (!skill) return;
    updateSkill(skillIndex, {
      files: skill.files.map((file, candidateIndex) =>
        candidateIndex === fileIndex ? { ...file, ...patch } : file,
      ),
    });
  }

  return (
    <section className="space-y-3" data-testid="agent-skills-section">
      <div className="flex items-start justify-between gap-3">
        <div>
          <div className="flex items-center gap-2">
            <GraduationCap className="h-4 w-4 text-muted-foreground" />
            <h3 className="text-sm font-medium text-foreground">Skills</h3>
            {status}
          </div>
          <p className="mt-1 text-xs text-muted-foreground">{description}</p>
        </div>
        <div className="flex items-center gap-2">
          {beforeAddAction}
          <Button
            data-testid="agent-skill-add"
            disabled={disabled}
            onClick={() => {
              skillKeys.current.push(crypto.randomUUID());
              fileKeys.current.push([crypto.randomUUID()]);
              onChange([...value, emptySkill()]);
            }}
            size="xs"
            type="button"
            variant="outline"
          >
            <Plus className="h-3.5 w-3.5" />
            Add skill
          </Button>
        </div>
      </div>

      {value.length === 0 ? (
        <div className="rounded-xl border border-dashed border-border/70 px-4 py-3 text-xs text-muted-foreground">
          {emptyMessage}
        </div>
      ) : (
        <div className="space-y-3">
          {value.map((skill, skillIndex) => (
            <div
              className="space-y-3 rounded-xl border border-border/70 bg-muted/10 p-3"
              data-testid={`agent-skill-${skillIndex}`}
              key={skillKeys.current[skillIndex]}
            >
              <div className="flex items-start gap-2">
                <div className="grid min-w-0 flex-1 gap-3 sm:grid-cols-2">
                  <div className="space-y-1.5">
                    <label
                      className="text-xs font-medium text-foreground"
                      htmlFor={`agent-skill-name-${skillIndex}`}
                    >
                      Skill name
                    </label>
                    <Input
                      autoCapitalize="off"
                      autoCorrect="off"
                      data-testid={`agent-skill-name-${skillIndex}`}
                      disabled={disabled}
                      id={`agent-skill-name-${skillIndex}`}
                      onChange={(event) =>
                        updateSkill(skillIndex, {
                          name: event.target.value.trim(),
                        })
                      }
                      placeholder="campaign-analysis"
                      spellCheck={false}
                      value={skill.name}
                    />
                    <p className="text-xs text-muted-foreground">
                      Lowercase letters, numbers, and hyphens.
                    </p>
                  </div>
                  <div className="space-y-1.5">
                    <label
                      className="text-xs font-medium text-foreground"
                      htmlFor={`agent-skill-description-${skillIndex}`}
                    >
                      Description
                    </label>
                    <Input
                      data-testid={`agent-skill-description-${skillIndex}`}
                      disabled={disabled}
                      id={`agent-skill-description-${skillIndex}`}
                      onChange={(event) =>
                        updateSkill(skillIndex, {
                          description: event.target.value,
                        })
                      }
                      placeholder="When this Skill should be used."
                      value={skill.description}
                    />
                  </div>
                </div>
                <Button
                  aria-label={`Remove skill ${skill.name || skillIndex + 1}`}
                  className="mt-6"
                  disabled={disabled}
                  onClick={() => {
                    skillKeys.current.splice(skillIndex, 1);
                    fileKeys.current.splice(skillIndex, 1);
                    onChange(
                      value.filter(
                        (_, candidateIndex) => candidateIndex !== skillIndex,
                      ),
                    );
                  }}
                  size="icon-xs"
                  type="button"
                  variant="ghost"
                >
                  <Trash2 className="h-4 w-4" />
                </Button>
              </div>

              <div className="space-y-2">
                <div className="flex items-center justify-between gap-3">
                  <div className="flex items-center gap-2">
                    <FileText className="h-3.5 w-3.5 text-muted-foreground" />
                    <p className="text-xs font-medium text-foreground">Files</p>
                  </div>
                  <Button
                    disabled={disabled}
                    onClick={() => {
                      fileKeys.current[skillIndex]?.push(crypto.randomUUID());
                      updateSkill(skillIndex, {
                        files: [...skill.files, { path: "", content: "" }],
                      });
                    }}
                    size="xs"
                    type="button"
                    variant="ghost"
                  >
                    <Plus className="h-3.5 w-3.5" />
                    Add file
                  </Button>
                </div>
                {skill.files.map((file, fileIndex) => (
                  <div
                    className="space-y-2 rounded-lg border border-border/60 bg-background/60 p-3"
                    key={fileKeys.current[skillIndex]?.[fileIndex]}
                  >
                    <div className="flex items-center gap-2">
                      <Input
                        aria-label={`File path ${fileIndex + 1}`}
                        autoCapitalize="off"
                        autoCorrect="off"
                        className="font-mono text-xs"
                        data-testid={`agent-skill-file-path-${skillIndex}-${fileIndex}`}
                        disabled={disabled || file.path === "SKILL.md"}
                        onChange={(event) =>
                          updateFile(skillIndex, fileIndex, {
                            path: event.target.value.trim(),
                          })
                        }
                        placeholder="references/example.md"
                        spellCheck={false}
                        value={file.path}
                      />
                      <Button
                        aria-label={`Remove file ${file.path || fileIndex + 1}`}
                        disabled={disabled || file.path === "SKILL.md"}
                        onClick={() => {
                          fileKeys.current[skillIndex]?.splice(fileIndex, 1);
                          updateSkill(skillIndex, {
                            files: skill.files.filter(
                              (_, candidateIndex) =>
                                candidateIndex !== fileIndex,
                            ),
                          });
                        }}
                        size="icon-xs"
                        type="button"
                        variant="ghost"
                      >
                        <Trash2 className="h-4 w-4" />
                      </Button>
                    </div>
                    <Textarea
                      aria-label={`${file.path || `File ${fileIndex + 1}`} content`}
                      className="min-h-28 resize-y font-mono text-xs"
                      data-testid={`agent-skill-file-content-${skillIndex}-${fileIndex}`}
                      disabled={disabled}
                      onChange={(event) =>
                        updateFile(skillIndex, fileIndex, {
                          content: event.target.value,
                        })
                      }
                      placeholder={
                        file.path === "SKILL.md"
                          ? "---\nname: campaign-analysis\ndescription: Analyze campaign performance.\n---"
                          : "Supporting instructions"
                      }
                      spellCheck={false}
                      value={file.content}
                    />
                  </div>
                ))}
              </div>
            </div>
          ))}
        </div>
      )}
    </section>
  );
}
