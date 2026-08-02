import { expect, test, type Locator, type Page } from "@playwright/test";

import { waitForAnimations } from "../helpers/animations";
import { installMockBridge, TEST_IDENTITIES } from "../helpers/bridge";

const SHOTS = "test-results/agent-template-update-screenshots";
const TEMPLATE_ID = "campaign-analyst-template";
const TEMPLATE_NAME = "Campaign Analyst";
const PREVIOUS_TEMPLATE_VERSION =
  "7d91f42b01481f4f36d7f3eca85bc50f684b09aa5ac9df16848e67bb0d64ae21";
const INITIAL_TEMPLATE_INSTRUCTIONS =
  "Analyze campaign performance and explain what changed.";
const CAMPAIGN_SKILL = {
  name: "campaign-analysis",
  description: "Compare campaign performance with the prior period.",
  files: [
    {
      path: "SKILL.md",
      content:
        "---\nname: campaign-analysis\ndescription: Compare campaign performance with the prior period.\n---\n\n# Campaign analysis\n\nCompare campaign performance with the prior period.",
    },
  ],
};

const LINKED_AGENTS = [
  {
    pubkey: TEST_IDENTITIES.alice.pubkey,
    name: "Atlas",
    personaId: TEMPLATE_ID,
    personaVersion: PREVIOUS_TEMPLATE_VERSION,
    status: "running" as const,
    channelNames: ["agents"],
    backend: { type: "local" as const },
    systemPrompt: INITIAL_TEMPLATE_INSTRUCTIONS,
    skills: [CAMPAIGN_SKILL],
    skillsChangedForAgent: true,
  },
  {
    pubkey: TEST_IDENTITIES.bob.pubkey,
    name: "Beacon",
    personaId: TEMPLATE_ID,
    personaVersion: PREVIOUS_TEMPLATE_VERSION,
    status: "stopped" as const,
    channelNames: ["agents"],
    backend: { type: "local" as const },
    systemPrompt: INITIAL_TEMPLATE_INSTRUCTIONS,
    skills: [CAMPAIGN_SKILL],
  },
  {
    pubkey: TEST_IDENTITIES.charlie.pubkey,
    name: "Comet",
    personaId: TEMPLATE_ID,
    personaVersion: PREVIOUS_TEMPLATE_VERSION,
    status: "running" as const,
    channelNames: ["agents"],
    backend: {
      type: "provider" as const,
      id: "blox",
      config: {},
    },
    systemPrompt: INITIAL_TEMPLATE_INSTRUCTIONS,
    skills: [CAMPAIGN_SKILL],
  },
  {
    pubkey: TEST_IDENTITIES.outsider.pubkey,
    name: "Drift",
    personaId: TEMPLATE_ID,
    personaVersion: PREVIOUS_TEMPLATE_VERSION,
    status: "stopped" as const,
    channelNames: ["agents"],
    backend: { type: "local" as const },
    systemPrompt: INITIAL_TEMPLATE_INSTRUCTIONS,
    skills: [CAMPAIGN_SKILL],
  },
];

async function capture(
  page: Page,
  subject: Locator,
  filename: string,
  animationTimeoutMs = 1_000,
) {
  await waitForAnimations(page, animationTimeoutMs);
  await subject.screenshot({ path: `${SHOTS}/${filename}` });
}

async function capturePage(page: Page, filename: string) {
  await waitForAnimations(page);
  await page.screenshot({ path: `${SHOTS}/${filename}` });
}

async function openTemplateEditor(page: Page) {
  await page.goto("/", { waitUntil: "domcontentloaded" });
  await page.getByTestId("open-agents-view").click();
  await page
    .getByRole("button", {
      exact: true,
      name: `${TEMPLATE_NAME} agent profile`,
    })
    .click();
  await page.getByTestId("user-profile-edit-agent").click();
  await page
    .getByTestId("agent-edit-scope-dialog")
    .getByTestId("agent-edit-scope-template")
    .click();
  const templateEditor = page.getByTestId("persona-dialog");
  await expect(templateEditor).toBeVisible({ timeout: 10_000 });
  return templateEditor;
}

test.describe("agent template update screenshots", () => {
  test.use({ viewport: { width: 1280, height: 900 } });

  test.beforeEach(async ({ page }, testInfo) => {
    page.on("pageerror", (error) => {
      console.error(
        "PAGE ERROR:",
        error.message,
        error.stack?.split("\n").slice(0, 5).join("\n"),
      );
    });
    page.on("console", (message) => {
      if (message.type() === "error") {
        console.error("CONSOLE ERROR:", message.text().slice(0, 500));
      }
    });

    if (testInfo.title.includes("dark compact")) {
      await page.addInitScript(() => {
        window.localStorage.setItem("buzz-theme", "github-dark");
      });
    }

    await installMockBridge(page, {
      globalAgentConfig: {
        provider: "anthropic",
        model: "claude-opus-4-5",
        env_vars: { ANTHROPIC_API_KEY: "test-only-placeholder" },
      },
      managedAgents: LINKED_AGENTS,
      agentTemplateUpdateStageDelayMs: 700,
      agentTemplateUpdateRecoveries: testInfo.title.includes(
        "interrupted update recovery",
      )
        ? [
            {
              transactionId: "recovery-transaction-1",
              templateId: TEMPLATE_ID,
              stage: "after_candidate_ready",
              recovery: "restore previous version",
              agents: LINKED_AGENTS.slice(0, 2).map((agent) => ({
                pubkey: agent.pubkey,
                name: agent.name,
              })),
              requiresAttention: true,
              detail:
                "An updated agent may have accepted work. Buzz blocked the affected agents instead of guessing which version owns that work.",
            },
          ]
        : undefined,
      personas: [
        {
          id: TEMPLATE_ID,
          displayName: TEMPLATE_NAME,
          systemPrompt: INITIAL_TEMPLATE_INSTRUCTIONS,
          skills: [CAMPAIGN_SKILL],
          updatedAt: "2026-07-15T12:00:00.000Z",
        },
      ],
    });
  });

  test("shows the complete linked-agent edit and update journey", async ({
    page,
  }) => {
    await page.goto("/", { waitUntil: "domcontentloaded" });
    await expect(page.getByTestId("open-agents-view")).toBeVisible({
      timeout: 10_000,
    });
    await page.getByTestId("open-agents-view").click();
    await expect(page.getByTestId("agents-library-personas")).toBeVisible({
      timeout: 10_000,
    });

    await page
      .getByRole("button", {
        exact: true,
        name: `${TEMPLATE_NAME} agent profile`,
      })
      .click();
    await expect(page.getByTestId("user-profile-panel")).toBeVisible({
      timeout: 10_000,
    });
    await page.getByTestId("user-profile-edit-agent").click();

    const scopeChooser = page.getByTestId("agent-edit-scope-dialog");
    await expect(scopeChooser).toBeVisible({ timeout: 10_000 });
    await expect(scopeChooser).toContainText("Edit Atlas");
    await expect(scopeChooser).toContainText("Used by 4 agents");
    for (const agent of LINKED_AGENTS) {
      await expect(scopeChooser).toContainText(agent.name);
    }
    await capture(page, scopeChooser, "01-profile-scope-chooser.png");

    await scopeChooser.getByTestId("agent-edit-scope-template").click();

    const templateEditor = page.getByTestId("persona-dialog");
    await expect(templateEditor).toBeVisible({ timeout: 10_000 });
    await expect(
      templateEditor.getByRole("heading", {
        name: TEMPLATE_NAME,
        exact: true,
      }),
    ).toBeVisible();
    await expect(page.locator("#persona-display-name")).toHaveValue(
      TEMPLATE_NAME,
    );
    const impactPreview = templateEditor.getByTestId("template-impact-preview");
    const identitySection = templateEditor.locator("#persona-identity-section");
    const templateContents = templateEditor.getByTestId(
      "template-contents-nav",
    );
    const editorInstructions = templateEditor.locator(
      "#persona-instructions-section",
    );
    await expect(impactPreview).toContainText("Used by 4 agents");
    for (const agent of LINKED_AGENTS) {
      await expect(impactPreview).toContainText(agent.name);
    }
    const [identityBox, impactBox, instructionsBox, contentsBox] =
      await Promise.all([
        identitySection.boundingBox(),
        impactPreview.boundingBox(),
        editorInstructions.boundingBox(),
        templateContents.boundingBox(),
      ]);
    if (!identityBox || !impactBox || !instructionsBox || !contentsBox) {
      throw new Error("Template editor hierarchy did not render.");
    }
    expect(identityBox.y + identityBox.height).toBeLessThanOrEqual(impactBox.y);
    expect(impactBox.y + impactBox.height).toBeLessThanOrEqual(
      instructionsBox.y,
    );
    expect(instructionsBox.y + instructionsBox.height).toBeLessThanOrEqual(
      contentsBox.y,
    );
    await capture(
      page,
      templateEditor,
      "02-template-editor-affected-agent-preview.png",
    );

    await expect(templateContents).toContainText("Template setup");
    await expect(templateContents).toContainText("AI configuration");
    await expect(templateContents).toContainText("1 Skill");
    await expect(templateContents).toContainText("0 requirements");
    await expect(templateContents).toContainText("Behavior");
    await expect(templateContents).toContainText(
      "stays on this device and is not included in a version",
    );
    await capture(
      page,
      templateContents,
      "02-template-editor-contents-map.png",
    );

    await templateContents
      .getByTestId("template-section-persona-model-section")
      .click();
    const modelSection = templateEditor.locator("#persona-model-section");
    await expect(modelSection).toBeInViewport();
    await capture(page, modelSection, "02-template-editor-runtime-model.png");

    await templateContents
      .getByTestId("template-section-persona-tools-section")
      .click();
    const toolsSection = templateEditor.getByTestId("agent-tools-section");
    await expect(toolsSection).toBeInViewport();
    await capture(page, toolsSection, "02-template-editor-tools.png");

    await templateContents
      .getByTestId("template-section-persona-behavior-section")
      .click();
    await expect(page.locator("#persona-parallelism")).toBeVisible();
    const behaviorSection = templateEditor.locator("#persona-behavior-section");
    await capture(page, behaviorSection, "02-template-editor-behavior.png");
    await page.locator("#persona-parallelism").fill("4");

    await templateContents
      .getByTestId("template-section-persona-skills-section")
      .click();
    const skillsSection = templateEditor.getByTestId("agent-skills-section");
    await expect(skillsSection).toBeInViewport();
    await expect(templateEditor.getByTestId("agent-skill-name-0")).toHaveValue(
      "campaign-analysis",
    );
    await expect(
      templateEditor.getByTestId("agent-skill-description-0"),
    ).toHaveValue(CAMPAIGN_SKILL.description);
    await capture(
      page,
      skillsSection,
      "02-template-editor-portable-skills.png",
    );
    await templateEditor
      .getByTestId("agent-skill-description-0")
      .fill("Compare campaign performance and recommend the next action.");
    await templateEditor
      .getByTestId("agent-skill-file-content-0-0")
      .fill(
        "---\nname: campaign-analysis\ndescription: Compare campaign performance and recommend the next action.\n---\n\n# Campaign analysis\n\nCompare performance and recommend the next action.",
      );
    await page
      .locator("#persona-system-prompt")
      .fill(
        "Analyze campaign performance, explain what changed, and recommend the next action.",
      );
    await expect(templateEditor).toContainText(
      "Save keeps running agents unchanged. Publish lets you choose which agents to update.",
    );
    await expect(
      templateEditor.getByTestId("persona-dialog-save-template"),
    ).toHaveText("Save changes");
    const publishButton = templateEditor.getByTestId("persona-dialog-submit");
    await expect(publishButton).toHaveText("Publish version");
    await expect(publishButton).toBeEnabled({ timeout: 10_000 });
    const commandsBeforeSave = await page.evaluate(
      () => window.__BUZZ_E2E_COMMAND_LOG__?.length ?? 0,
    );
    await publishButton.click();
    await expect(templateEditor).not.toBeVisible({ timeout: 10_000 });

    const updateReview = page.getByTestId("template-publish-review");
    await expect(updateReview).toBeVisible({ timeout: 10_000 });
    await expect
      .poll(() =>
        page.evaluate((start) => {
          const commands = window.__BUZZ_E2E_COMMAND_LOG__ ?? [];
          return commands
            .slice(start)
            .some((entry) => entry.command === "update_managed_agent");
        }, commandsBeforeSave),
      )
      .toBe(false);
    await expect(updateReview).toContainText("Update Campaign Analyst");
    await expect(updateReview).toContainText("Choose who should get");
    await expect(updateReview).toContainText(
      "Buzz finishes current work before switching versions. If the update fails, it restores the previous version.",
    );
    await expect(updateReview).toContainText("Skills in this update");
    await expect(updateReview).toContainText("Changed campaign-analysis");
    await expect(updateReview).toContainText("Configuration in this update");
    await expect(updateReview).toContainText("Parallel work");
    await expect(updateReview).toContainText(
      "This agent keeps its own Skills.",
    );
    const instructionChanges = updateReview.getByTestId(
      "template-instruction-changes",
    );
    await expect(instructionChanges).toContainText(
      "Agent instructions in this update",
    );
    await expect(instructionChanges.getByText("Current")).toBeVisible();
    await expect(instructionChanges.getByText("New version")).toBeVisible();
    await expect(instructionChanges.locator("pre").nth(0)).toHaveText(
      INITIAL_TEMPLATE_INSTRUCTIONS,
    );
    await expect(instructionChanges.locator("pre").nth(1)).toHaveText(
      "Analyze campaign performance, explain what changed, and recommend the next action.",
    );
    await expect(updateReview).toContainText(
      "Remote agents cannot be updated safely in this version.",
    );
    await expect(
      updateReview.getByTestId(
        `template-update-agent-${TEST_IDENTITIES.alice.pubkey}`,
      ),
    ).toContainText("Atlas");
    const updateButton = updateReview.getByTestId(
      "template-publish-and-update",
    );
    await expect(updateButton).toHaveText("Update 3 agents");
    await capture(page, updateReview, "03-update-selection-review.png");

    await updateButton.click();
    await expect(
      updateReview.getByTestId("template-rollout-progress"),
    ).toBeVisible({ timeout: 2_000 });
    await expect(
      updateReview.getByRole("heading", {
        name: "Updating Campaign Analyst",
        exact: true,
      }),
    ).toBeVisible();
    await expect(updateReview).toContainText(
      "3 agents using Campaign Analyst are being updated.",
    );
    await expect(
      updateReview.getByTestId("template-rollout-progress"),
    ).toContainText("Preparing update");
    await expect(updateReview).toContainText(
      "This update will continue in the background if you close this window.",
    );
    await expect(
      updateReview.getByRole("button", { name: "Continue working" }),
    ).toBeEnabled();
    const [progressBox, instructionBox] = await Promise.all([
      updateReview.getByTestId("template-rollout-progress").boundingBox(),
      instructionChanges.boundingBox(),
    ]);
    if (!progressBox || !instructionBox) {
      throw new Error("Template rollout hierarchy did not render.");
    }
    expect(progressBox.y + progressBox.height).toBeLessThanOrEqual(
      instructionBox.y,
    );
    await capture(page, updateReview, "04-preparing-update.png", 75);

    await expect(
      updateReview.getByTestId("template-rollout-progress"),
    ).toContainText("Finishing current task");
    await capture(page, updateReview, "05-finishing-current-task.png", 75);

    await expect(
      updateReview.getByTestId("template-rollout-progress"),
    ).toContainText("Starting updated agent");
    await capture(page, updateReview, "06-starting-updated-agent.png", 75);

    await expect(
      updateReview.getByTestId("template-rollout-progress"),
    ).toContainText("Checking update");
    await capture(page, updateReview, "07-checking-update.png", 75);
    await updateReview
      .getByRole("button", { name: "Continue working" })
      .click();
    await expect(updateReview).not.toBeVisible();
    const backgroundNotice = page.getByText(
      "Campaign Analyst updated on 3 agents.",
      {
        exact: true,
      },
    );
    await expect(backgroundNotice).toBeVisible({ timeout: 10_000 });
    await capture(
      page,
      page
        .locator("[data-sonner-toast]")
        .filter({ hasText: "Campaign Analyst updated on 3 agents." }),
      "08-background-update-finished.png",
    );
    await page.getByRole("button", { name: "View results" }).click();

    await expect(
      updateReview.getByText("Campaign Analyst updated", { exact: true }),
    ).toBeVisible({ timeout: 10_000 });
    await expect(updateReview).toContainText("3 agents were updated.");
    await expect(updateReview).toContainText("Updated and ready");
    await expect(updateReview).toContainText(
      "Updated · starts on this version next time",
    );
    await capture(page, updateReview, "09-completed-update.png");
  });

  test("keeps the saved edit when version publishing fails", async ({
    page,
  }) => {
    await page.goto("/", { waitUntil: "domcontentloaded" });
    await page.evaluate(() => {
      if (window.__BUZZ_E2E__?.mock) {
        window.__BUZZ_E2E__.mock.publishAgentTemplateVersionError =
          "Mock Git failure";
      }
    });
    expect(
      await page.evaluate(
        () => window.__BUZZ_E2E__?.mock?.publishAgentTemplateVersionError,
      ),
    ).toBe("Mock Git failure");
    await page.getByTestId("open-agents-view").click();
    await page
      .getByRole("button", {
        exact: true,
        name: `${TEMPLATE_NAME} agent profile`,
      })
      .click();
    await page.getByTestId("user-profile-edit-agent").click();
    await page
      .getByTestId("agent-edit-scope-dialog")
      .getByTestId("agent-edit-scope-template")
      .click();

    const templateEditor = page.getByTestId("persona-dialog");
    const savedInstructions =
      "Keep the template edit even if publishing cannot reach Git.";
    await page.locator("#persona-system-prompt").fill(savedInstructions);
    await templateEditor.getByTestId("persona-dialog-submit").click();

    await expect(templateEditor).toBeVisible({ timeout: 10_000 });
    await expect(
      templateEditor.getByText("Template saved. Try again.", {
        exact: true,
      }),
    ).toBeVisible();
    await expect(
      templateEditor.getByText("Version wasn’t published", { exact: true }),
    ).toBeVisible();
    await expect(
      templateEditor.getByRole("button", { name: "Retry publishing" }),
    ).toBeVisible();
    await expect(page.locator("#persona-system-prompt")).toHaveValue(
      savedInstructions,
    );
    const [errorBox, actionsBox] = await Promise.all([
      templateEditor.getByTestId("persona-dialog-error").boundingBox(),
      templateEditor.getByTestId("persona-dialog-actions").boundingBox(),
    ]);
    if (!errorBox || !actionsBox) {
      throw new Error("Publish recovery footer did not render.");
    }
    expect(errorBox.y + errorBox.height).toBeLessThanOrEqual(actionsBox.y);
    await capture(page, templateEditor, "10-publish-failure.png");

    await page.evaluate(() => {
      if (window.__BUZZ_E2E__?.mock) {
        window.__BUZZ_E2E__.mock.publishAgentTemplateVersionError = undefined;
      }
    });
    await templateEditor
      .getByRole("button", { name: "Retry publishing" })
      .click();
    await expect(
      page
        .getByTestId("template-publish-review")
        .getByText("Update Campaign Analyst", { exact: true }),
    ).toBeVisible({ timeout: 10_000 });
  });

  test("saving the mutable template does not open an agent update", async ({
    page,
  }) => {
    await page.goto("/", { waitUntil: "domcontentloaded" });
    await page.getByTestId("open-agents-view").click();
    await page
      .getByRole("button", {
        exact: true,
        name: `${TEMPLATE_NAME} agent profile`,
      })
      .click();
    await page.getByTestId("user-profile-edit-agent").click();
    await page
      .getByTestId("agent-edit-scope-dialog")
      .getByTestId("agent-edit-scope-template")
      .click();

    const templateEditor = page.getByTestId("persona-dialog");
    await page
      .locator("#persona-system-prompt")
      .fill("Save this draft without publishing a template version.");
    const commandCount = await page.evaluate(
      () => window.__BUZZ_E2E_COMMAND_LOG__?.length ?? 0,
    );
    await templateEditor.getByTestId("persona-dialog-save-template").click();
    await expect(templateEditor).not.toBeVisible({ timeout: 10_000 });
    await expect(page.getByTestId("template-publish-review")).not.toBeVisible();

    const templateCommands = await page.evaluate((start) => {
      return (window.__BUZZ_E2E_COMMAND_LOG__ ?? [])
        .slice(start)
        .map((entry) => entry.command)
        .filter((command) =>
          [
            "update_persona",
            "publish_agent_template_version",
            "preview_agent_template_update",
          ].includes(command),
        );
    }, commandCount);
    expect(templateCommands).toEqual(["update_persona"]);
  });

  test("shows inline Skill validation and protects unsaved work", async ({
    page,
  }) => {
    const templateEditor = await openTemplateEditor(page);
    await templateEditor.getByTestId("agent-skill-add").click();

    const newSkillName = templateEditor.getByTestId("agent-skill-name-1");
    await expect(newSkillName).toHaveAttribute("aria-invalid", "true");
    await expect(newSkillName).toHaveValue("");
    await expect(
      templateEditor.getByRole("alert").filter({
        hasText: "Enter a Skill name.",
      }),
    ).toBeVisible();
    await capture(
      page,
      templateEditor.getByTestId("agent-skill-1"),
      "11-inline-skill-validation.png",
    );

    await templateEditor.getByRole("button", { name: "Cancel" }).click();
    const confirmation = page.getByTestId("persona-discard-confirmation");
    await expect(confirmation).toBeVisible();
    await expect(confirmation).toContainText(
      "Discard changes to Campaign Analyst?",
    );
    await capture(page, confirmation, "12-discard-unsaved-confirmation.png");
    await confirmation.getByRole("button", { name: "Keep editing" }).click();
    await expect(templateEditor).toBeVisible();
  });

  test("keeps desktop density in a dark compact window", async ({ page }) => {
    await page.setViewportSize({ width: 860, height: 720 });
    const templateEditor = await openTemplateEditor(page);
    await expect
      .poll(() =>
        page.evaluate(() =>
          document.documentElement.classList.contains("dark"),
        ),
      )
      .toBe(true);
    await expect(
      templateEditor.getByTestId("template-contents-nav"),
    ).toBeVisible();
    await expect(
      templateEditor.locator("#persona-instructions-section"),
    ).toBeInViewport();
    await capturePage(page, "13-dark-compact-template-editor.png");
  });

  test("keeps the editor usable at the minimum desktop window size", async ({
    page,
  }) => {
    await page.setViewportSize({ width: 800, height: 500 });
    const templateEditor = await openTemplateEditor(page);

    await expect(
      templateEditor.getByTestId("template-contents-nav"),
    ).toBeVisible();
    await expect(
      templateEditor.getByRole("button", { name: "Save changes" }),
    ).toBeVisible();
    await expect(
      templateEditor.getByRole("button", { name: "Publish version" }),
    ).toBeVisible();
    expect(
      await templateEditor.evaluate(
        (element) => element.scrollWidth <= element.clientWidth,
      ),
    ).toBe(true);

    await capturePage(page, "14-minimum-window-template-editor.png");
  });

  test("shows an interrupted update recovery path", async ({ page }) => {
    await page.goto("/", { waitUntil: "domcontentloaded" });
    await page.getByTestId("open-agents-view").click();

    const banner = page.getByTestId("agent-update-recovery-banner");
    await expect(banner).toBeVisible({ timeout: 10_000 });
    await expect(banner).toContainText("Agent updates are paused");
    await expect(banner).toContainText("Campaign Analyst");
    await expect(banner).toContainText("Atlas, Beacon");
    await capture(page, banner, "15-interrupted-update-recovery.png");

    await banner.getByTestId("agent-update-recovery-open").click();
    const confirmation = page.getByTestId("agent-update-recovery-confirmation");
    await expect(confirmation).toBeVisible();
    await expect(confirmation).toContainText(
      "Work accepted during that update may be incomplete.",
    );
    await expect(confirmation).toContainText("Restore Campaign Analyst?");
    await capture(page, confirmation, "16-interrupted-update-confirmation.png");

    await confirmation
      .getByRole("button", { name: "Stop and restore" })
      .click();
    await expect(banner).not.toBeVisible();
    await expect
      .poll(() =>
        page.evaluate(
          () =>
            window.__BUZZ_E2E_COMMAND_LOG__?.filter(
              (entry) =>
                entry.command === "restore_interrupted_agent_template_update",
            ).length ?? 0,
        ),
      )
      .toBe(1);
  });
});
