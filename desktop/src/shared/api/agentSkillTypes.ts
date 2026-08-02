import { parse as yamlParse, parseDocument as yamlParseDocument } from "yaml";

export type AgentSkillFile = {
  path: string;
  content: string;
};

export type AgentSkill = {
  name: string;
  description: string;
  files: AgentSkillFile[];
};

export type AgentSkillValidationIssue = {
  field:
    | "skills"
    | "name"
    | "description"
    | "files"
    | "file-path"
    | "file-content"
    | "skill-markdown";
  fileIndex?: number;
  message: string;
  skillIndex?: number;
};

const SKILL_NAME_RE = /^[a-z0-9](?:[a-z0-9-]{0,62}[a-z0-9])?$/u;
const MAX_SKILLS = 16;
const MAX_SKILL_DESCRIPTION_BYTES = 512;
const MAX_SKILL_FILES = 32;
const MAX_SKILL_PATH_BYTES = 240;
const MAX_SKILL_FILE_BYTES = 64 * 1_024;
const MAX_SKILL_TOTAL_BYTES = 128 * 1_024;
const RESERVED_SKILL_NAMES = new Set(["buzz-cli"]);
const UTF8_ENCODER = new TextEncoder();

function utf8Length(value: string): number {
  return UTF8_ENCODER.encode(value).byteLength;
}

function skillPathValid(path: string): boolean {
  if (
    path.length === 0 ||
    utf8Length(path) > MAX_SKILL_PATH_BYTES ||
    path.startsWith("/") ||
    path.includes("\\") ||
    path.includes("\0")
  ) {
    return false;
  }
  const segments = path.split("/");
  return segments.every(
    (segment) => segment.length > 0 && segment !== "." && segment !== "..",
  );
}

function looksLikePlaceholder(value: string): boolean {
  const normalized = value.toLowerCase();
  return [
    "example",
    "placeholder",
    "your-",
    "your_",
    "replace",
    "xxxx",
    "<",
    "${",
    "test-only",
  ].some((marker) => normalized.includes(marker));
}

function containsProbableToken(content: string, prefix: string): boolean {
  let remaining = content;
  let index = remaining.indexOf(prefix);
  while (index !== -1) {
    let candidate = "";
    for (const character of remaining.slice(index)) {
      if (!/[A-Za-z0-9_:-]/u.test(character) || candidate.length >= 160) {
        break;
      }
      candidate += character;
    }
    if (
      candidate.length >= prefix.length + 12 &&
      !looksLikePlaceholder(candidate)
    ) {
      return true;
    }
    remaining = remaining.slice(index + prefix.length);
    index = remaining.indexOf(prefix);
  }
  return false;
}

function containsObviousSecret(content: string): boolean {
  const uppercase = content.toUpperCase();
  if (
    uppercase.includes("-----BEGIN PRIVATE KEY-----") ||
    uppercase.includes("-----BEGIN RSA PRIVATE KEY-----") ||
    uppercase.includes("-----BEGIN OPENSSH PRIVATE KEY-----")
  ) {
    return true;
  }
  for (const prefix of [
    "nsec1",
    "sk_live_",
    "sk-prod-",
    "ghp_",
    "github_pat_",
    "xoxb-",
    "xoxp-",
    "AKIA",
  ]) {
    if (containsProbableToken(content, prefix)) return true;
  }
  for (const line of content.split(/\r?\n/u)) {
    const trimmed = line
      .trim()
      .replace(/^export\s+/u, "")
      .trim();
    const separator = trimmed.search(/[=:]/u);
    if (separator === -1) continue;
    const key = trimmed
      .slice(0, separator)
      .replace(/["'\s]/gu, "")
      .toUpperCase();
    if (
      !["API_KEY", "TOKEN", "PASSWORD", "SECRET", "PRIVATE_KEY"].some(
        (marker) => key.includes(marker),
      )
    ) {
      continue;
    }
    const value = trimmed
      .slice(separator + 1)
      .trim()
      .replace(/^["']|["']$/gu, "");
    if (utf8Length(value) >= 8 && !looksLikePlaceholder(value)) return true;
  }
  return false;
}

function frontmatterValid(skill: AgentSkill, content: string): boolean {
  const normalized = content.replace(/\r\n/gu, "\n");
  if (!normalized.startsWith("---\n")) return false;
  const closing = normalized.indexOf("\n---\n", 4);
  if (closing === -1) return false;
  try {
    const metadata: unknown = yamlParse(normalized.slice(4, closing));
    if (
      typeof metadata !== "object" ||
      metadata === null ||
      Array.isArray(metadata)
    ) {
      return false;
    }
    const record = metadata as Record<string, unknown>;
    return (
      typeof record.name === "string" &&
      record.name.trim() === skill.name &&
      typeof record.description === "string" &&
      record.description.trim().length > 0 &&
      record.description.trim() === skill.description.trim()
    );
  } catch {
    return false;
  }
}

/**
 * Keep the duplicated summary fields in `SKILL.md` aligned with the editor
 * without discarding other frontmatter keys or the instruction body.
 */
export function synchronizeSkillMarkdown(
  content: string,
  name: string,
  description: string,
): string {
  const normalized = content.replace(/\r\n/gu, "\n");
  const closing = normalized.startsWith("---\n")
    ? normalized.indexOf("\n---\n", 4)
    : -1;
  if (closing !== -1) {
    try {
      const document = yamlParseDocument(normalized.slice(4, closing));
      if (document.errors.length === 0) {
        document.set("name", name);
        document.set("description", description);
        return `---\n${document.toString().trimEnd()}\n---\n${normalized.slice(closing + 5)}`;
      }
    } catch {
      // Fall through and preserve the original text as the instruction body.
    }
  }
  const body = normalized.length > 0 ? `\n${normalized}` : "";
  return `---\nname: ${JSON.stringify(name)}\ndescription: ${JSON.stringify(description)}\n---\n${body}`;
}

export function agentSkillsValidationIssue(
  skills: readonly AgentSkill[],
): AgentSkillValidationIssue | null {
  if (skills.length > MAX_SKILLS) {
    return {
      field: "skills",
      message: `Templates can include up to ${MAX_SKILLS} Skills.`,
    };
  }

  const names = new Set<string>();
  let totalLength = 0;
  for (const [skillIndex, skill] of skills.entries()) {
    const name = skill.name.trim();
    if (name.length === 0) {
      return {
        field: "name",
        message: "Enter a Skill name.",
        skillIndex,
      };
    }
    if (!SKILL_NAME_RE.test(name)) {
      return {
        field: "name",
        message: "Use lowercase letters, numbers, and hyphens.",
        skillIndex,
      };
    }
    if (RESERVED_SKILL_NAMES.has(name)) {
      return {
        field: "name",
        message: `"${name}" is reserved by Buzz. Choose another Skill name.`,
        skillIndex,
      };
    }
    if (names.has(name)) {
      return {
        field: "name",
        message: `"${name}" is already used by another Skill.`,
        skillIndex,
      };
    }
    if (
      skill.description.trim().length === 0 ||
      utf8Length(skill.description) > MAX_SKILL_DESCRIPTION_BYTES
    ) {
      return {
        field: "description",
        message: `Add a description up to ${MAX_SKILL_DESCRIPTION_BYTES} bytes.`,
        skillIndex,
      };
    }
    if (skill.files.length === 0 || skill.files.length > MAX_SKILL_FILES) {
      return {
        field: "files",
        message: `Include 1 to ${MAX_SKILL_FILES} files.`,
        skillIndex,
      };
    }
    names.add(name);

    const paths = new Set<string>();
    let skillMarkdown: string | null = null;
    let skillMarkdownIndex: number | undefined;
    for (const [fileIndex, file] of skill.files.entries()) {
      const path = file.path;
      if (!skillPathValid(path)) {
        return {
          field: "file-path",
          fileIndex,
          message: "Use a safe relative path, such as references/example.md.",
          skillIndex,
        };
      }
      if (paths.has(path)) {
        return {
          field: "file-path",
          fileIndex,
          message: `"${path}" is already used in this Skill.`,
          skillIndex,
        };
      }
      if (utf8Length(file.content) > MAX_SKILL_FILE_BYTES) {
        return {
          field: "file-content",
          fileIndex,
          message: `Keep this file smaller than ${MAX_SKILL_FILE_BYTES / 1_024} KiB.`,
          skillIndex,
        };
      }
      if (containsObviousSecret(file.content)) {
        return {
          field: "file-content",
          fileIndex,
          message:
            "This file appears to contain a secret. Use a Connection or agent environment setting instead.",
          skillIndex,
        };
      }
      paths.add(path);
      totalLength += utf8Length(path) + utf8Length(file.content);
      if (totalLength > MAX_SKILL_TOTAL_BYTES) {
        return {
          field: "file-content",
          fileIndex,
          message: `Skills can contain up to ${MAX_SKILL_TOTAL_BYTES / 1_024} KiB in total.`,
          skillIndex,
        };
      }
      if (path === "SKILL.md") {
        skillMarkdown = file.content;
        skillMarkdownIndex = fileIndex;
      }
    }
    if (skillMarkdown === null || !frontmatterValid(skill, skillMarkdown)) {
      return {
        field: "skill-markdown",
        fileIndex: skillMarkdownIndex,
        message:
          "SKILL.md frontmatter must match the Skill name and description.",
        skillIndex,
      };
    }
  }
  return null;
}

export function agentSkillsValidationError(
  skills: readonly AgentSkill[],
): string | null {
  return agentSkillsValidationIssue(skills)?.message ?? null;
}

export function agentSkillsValid(skills: readonly AgentSkill[]): boolean {
  return agentSkillsValidationIssue(skills) === null;
}

function isAgentSkillFile(value: unknown): value is AgentSkillFile {
  return (
    typeof value === "object" &&
    value !== null &&
    "path" in value &&
    typeof value.path === "string" &&
    "content" in value &&
    typeof value.content === "string"
  );
}

function isAgentSkill(value: unknown): value is AgentSkill {
  return (
    typeof value === "object" &&
    value !== null &&
    "name" in value &&
    typeof value.name === "string" &&
    "description" in value &&
    typeof value.description === "string" &&
    "files" in value &&
    Array.isArray(value.files) &&
    value.files.every(isAgentSkillFile)
  );
}

/** Parse an untrusted wire value without admitting partial or unsafe Skills. */
export function parseAgentSkills(value: unknown): AgentSkill[] {
  if (!Array.isArray(value) || !value.every(isAgentSkill)) return [];
  const skills = value.map((skill) => ({
    name: skill.name,
    description: skill.description,
    files: skill.files.map((file) => ({ ...file })),
  }));
  return agentSkillsValid(skills) ? skills : [];
}
