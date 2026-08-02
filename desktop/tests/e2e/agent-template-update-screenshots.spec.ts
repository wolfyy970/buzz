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

test.describe("agent template update screenshots", () => {
  test.use({ viewport: { width: 1280, height: 900 } });

  test.beforeEach(async ({ page }) => {
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

    await installMockBridge(page, {
      globalAgentConfig: {
        provider: "anthropic",
        model: "claude-opus-4-5",
        env_vars: { ANTHROPIC_API_KEY: "test-only-placeholder" },
      },
      managedAgents: LINKED_AGENTS,
      agentTemplateUpdateStageDelayMs: 700,
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
    await expect(page.locator("#persona-display-name")).toHaveValue(
      TEMPLATE_NAME,
    );
    const impactPreview = templateEditor.getByTestId("template-impact-preview");
    await expect(impactPreview).toContainText("Used by 4 agents");
    for (const agent of LINKED_AGENTS) {
      await expect(impactPreview).toContainText(agent.name);
    }
    await capture(
      page,
      templateEditor,
      "02-template-editor-affected-agent-preview.png",
    );

    const skillsSection = templateEditor.getByTestId("agent-skills-section");
    await skillsSection.scrollIntoViewIfNeeded();
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
    await expect(
      templateEditor.getByTestId("persona-dialog-template-version-notice"),
    ).toHaveText(
      "Publishing creates a version you can use to update agents. Running agents do not change.",
    );
    await expect(
      templateEditor.getByTestId("persona-dialog-save-template"),
    ).toHaveText("Save template");
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
    await expect(updateReview).toContainText(
      `Update agents using ${TEMPLATE_NAME}?`,
    );
    await expect(updateReview).toContainText("Choose which agents");
    await expect(updateReview).toContainText(
      "Safe to update now. Buzz stops new work and lets the current task finish. If it cannot, the new version recovers it after the update.",
    );
    await expect(updateReview).toContainText("Skills in this update");
    await expect(updateReview).toContainText("Changed campaign-analysis");
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
      updateReview.getByTestId("template-rollout-progress"),
    ).toContainText("Preparing update");
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

    await expect(
      updateReview.getByTestId("template-rollout-progress"),
    ).toContainText("Updated");
    await capture(page, updateReview, "08-updated.png", 75);

    await expect(
      updateReview.getByText("Agents updated", { exact: true }),
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

    await expect(templateEditor).not.toBeVisible({ timeout: 10_000 });
    await expect(
      page.getByText("Template saved. Version wasn’t published. Try again.", {
        exact: true,
      }),
    ).toBeVisible();

    await page.getByTestId("user-profile-edit-agent").click();
    await page
      .getByTestId("agent-edit-scope-dialog")
      .getByTestId("agent-edit-scope-template")
      .click();
    await expect(page.locator("#persona-system-prompt")).toHaveValue(
      savedInstructions,
    );
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
});
