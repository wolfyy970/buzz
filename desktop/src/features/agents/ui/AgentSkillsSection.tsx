import { FileText, GraduationCap, Plus, Trash2 } from "lucide-react";
import * as React from "react";

import {
  type AgentSkill,
  type AgentSkillFile,
  type AgentSkillValidationIssue,
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
  description = "Reusable instructions and files packaged with this template.",
  disabled,
  emptyMessage = "This template does not include any Skills.",
  headingLevel = "h3",
  validationIssue,
  status,
  onChange,
  value,
}: {
  beforeAddAction?: React.ReactNode;
  description?: string;
  disabled: boolean;
  emptyMessage?: string;
  headingLevel?: "h3" | "h4";
  validationIssue?: AgentSkillValidationIssue | null;
  status?: React.ReactNode;
  onChange: (value: AgentSkill[]) => void;
  value: AgentSkill[];
}) {
  const Heading = headingLevel;
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

  function issueFor(
    skillIndex: number,
    field: AgentSkillValidationIssue["field"],
    fileIndex?: number,
  ) {
    if (
      validationIssue?.skillIndex !== skillIndex ||
      validationIssue.field !== field ||
      validationIssue.fileIndex !== fileIndex
    ) {
      return null;
    }
    return validationIssue;
  }

  return (
    <section
      aria-describedby={
        validationIssue?.field === "skills"
          ? "agent-skills-validation-error"
          : undefined
      }
      className="space-y-3"
      data-testid="agent-skills-section"
    >
      <div className="flex items-start justify-between gap-3">
        <div>
          <div className="flex items-center gap-2">
            <GraduationCap className="h-4 w-4 text-muted-foreground" />
            <Heading className="text-base font-semibold text-foreground">
              Skills
            </Heading>
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
            className="min-h-9"
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
              <div className="flex items-center justify-between gap-3">
                <p className="text-sm font-medium text-foreground">
                  Skill {skillIndex + 1}
                </p>
                <Button
                  aria-label={`Remove skill ${skill.name || skillIndex + 1}`}
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
                  className="size-9"
                  size="icon"
                  type="button"
                  variant="ghost"
                >
                  <Trash2 className="h-4 w-4" />
                </Button>
              </div>

              <div className="space-y-1.5">
                {(() => {
                  const issue = issueFor(skillIndex, "name");
                  const helpId = `agent-skill-name-help-${skillIndex}`;
                  const errorId = `agent-skill-name-error-${skillIndex}`;
                  return (
                    <>
                      <label
                        className="text-xs font-medium text-foreground"
                        htmlFor={`agent-skill-name-${skillIndex}`}
                      >
                        Skill name
                      </label>
                      <Input
                        aria-describedby={issue ? errorId : helpId}
                        aria-invalid={Boolean(issue)}
                        autoComplete="off"
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
                        placeholder="e.g. campaign-analysis"
                        spellCheck={false}
                        value={skill.name}
                      />
                      <p className="text-xs text-muted-foreground" id={helpId}>
                        Use lowercase letters, numbers, and hyphens.
                      </p>
                      {issue ? (
                        <p
                          className="text-xs text-destructive"
                          id={errorId}
                          role="alert"
                        >
                          {issue.message}
                        </p>
                      ) : null}
                    </>
                  );
                })()}
              </div>

              <div className="space-y-1.5">
                {(() => {
                  const issue = issueFor(skillIndex, "description");
                  const errorId = `agent-skill-description-error-${skillIndex}`;
                  return (
                    <>
                      <label
                        className="text-xs font-medium text-foreground"
                        htmlFor={`agent-skill-description-${skillIndex}`}
                      >
                        When should this skill be used?
                      </label>
                      <Textarea
                        aria-describedby={issue ? errorId : undefined}
                        aria-invalid={Boolean(issue)}
                        className="min-h-16 resize-y"
                        data-testid={`agent-skill-description-${skillIndex}`}
                        disabled={disabled}
                        id={`agent-skill-description-${skillIndex}`}
                        onChange={(event) =>
                          updateSkill(skillIndex, {
                            description: event.target.value,
                          })
                        }
                        placeholder="Describe when the agent should use this skill."
                        value={skill.description}
                      />
                      {issue ? (
                        <p
                          className="text-xs text-destructive"
                          id={errorId}
                          role="alert"
                        >
                          {issue.message}
                        </p>
                      ) : null}
                    </>
                  );
                })()}
              </div>

              <div className="space-y-2">
                <div className="flex items-center justify-between gap-3">
                  <div className="flex items-center gap-2">
                    <FileText className="h-3.5 w-3.5 text-muted-foreground" />
                    <p className="text-xs font-medium text-foreground">Files</p>
                  </div>
                  <Button
                    className="min-h-9"
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
                {validationIssue?.skillIndex === skillIndex &&
                (validationIssue.field === "files" ||
                  (validationIssue.field === "skill-markdown" &&
                    validationIssue.fileIndex === undefined)) ? (
                  <p className="text-xs text-destructive" role="alert">
                    {validationIssue.message}
                  </p>
                ) : null}
                {skill.files.map((file, fileIndex) => (
                  <div
                    className="space-y-2 rounded-lg border border-border/60 bg-background/60 p-3"
                    key={fileKeys.current[skillIndex]?.[fileIndex]}
                  >
                    <div className="flex items-start gap-2">
                      {file.path === "SKILL.md" ? (
                        <div
                          className="flex min-h-9 flex-1 items-center gap-2 rounded-md bg-muted/50 px-3 font-mono text-xs text-foreground"
                          data-testid={`agent-skill-file-path-${skillIndex}-${fileIndex}`}
                        >
                          <FileText className="size-3.5 text-muted-foreground" />
                          SKILL.md
                          <span className="ml-auto font-sans text-2xs text-muted-foreground">
                            Required
                          </span>
                        </div>
                      ) : (
                        <>
                          {(() => {
                            const issue = issueFor(
                              skillIndex,
                              "file-path",
                              fileIndex,
                            );
                            const errorId = `agent-skill-file-path-error-${skillIndex}-${fileIndex}`;
                            return (
                              <div className="min-w-0 flex-1 space-y-1.5">
                                <Input
                                  aria-describedby={issue ? errorId : undefined}
                                  aria-invalid={Boolean(issue)}
                                  aria-label={`File path ${fileIndex + 1}`}
                                  autoComplete="off"
                                  autoCapitalize="off"
                                  autoCorrect="off"
                                  className="font-mono text-xs"
                                  data-testid={`agent-skill-file-path-${skillIndex}-${fileIndex}`}
                                  disabled={disabled}
                                  onChange={(event) =>
                                    updateFile(skillIndex, fileIndex, {
                                      path: event.target.value.trim(),
                                    })
                                  }
                                  placeholder="references/example.md"
                                  spellCheck={false}
                                  value={file.path}
                                />
                                {issue ? (
                                  <p
                                    className="text-xs text-destructive"
                                    id={errorId}
                                    role="alert"
                                  >
                                    {issue.message}
                                  </p>
                                ) : null}
                              </div>
                            );
                          })()}
                          <Button
                            aria-label={`Remove file ${file.path || fileIndex + 1}`}
                            disabled={disabled}
                            onClick={() => {
                              fileKeys.current[skillIndex]?.splice(
                                fileIndex,
                                1,
                              );
                              updateSkill(skillIndex, {
                                files: skill.files.filter(
                                  (_, candidateIndex) =>
                                    candidateIndex !== fileIndex,
                                ),
                              });
                            }}
                            className="size-9"
                            size="icon"
                            type="button"
                            variant="ghost"
                          >
                            <Trash2 className="h-4 w-4" />
                          </Button>
                        </>
                      )}
                    </div>
                    {(() => {
                      const contentIssue =
                        issueFor(skillIndex, "file-content", fileIndex) ??
                        issueFor(skillIndex, "skill-markdown", fileIndex);
                      const errorId = `agent-skill-file-content-error-${skillIndex}-${fileIndex}`;
                      return (
                        <>
                          <Textarea
                            aria-describedby={
                              contentIssue ? errorId : undefined
                            }
                            aria-invalid={Boolean(contentIssue)}
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
                          {contentIssue ? (
                            <p
                              className="text-xs text-destructive"
                              id={errorId}
                              role="alert"
                            >
                              {contentIssue.message}
                            </p>
                          ) : null}
                        </>
                      );
                    })()}
                  </div>
                ))}
              </div>
            </div>
          ))}
        </div>
      )}
      {validationIssue?.field === "skills" ? (
        <p
          className="text-xs text-destructive"
          id="agent-skills-validation-error"
          role="alert"
        >
          {validationIssue.message}
        </p>
      ) : null}
    </section>
  );
}
