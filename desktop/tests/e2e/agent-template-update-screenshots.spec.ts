import { expect, test, type Locator, type Page } from "@playwright/test";

import { waitForAnimations } from "../helpers/animations";
import { installMockBridge, TEST_IDENTITIES } from "../helpers/bridge";

const SHOTS = "test-results/agent-template-update-screenshots";
const TEMPLATE_ID = "campaign-analyst-template";
const TEMPLATE_NAME = "Campaign Analyst";
const PREVIOUS_TEMPLATE_VERSION =
  "7d91f42b01481f4f36d7f3eca85bc50f684b09aa5ac9df16848e67bb0d64ae21";

const LINKED_AGENTS = [
  {
    pubkey: TEST_IDENTITIES.alice.pubkey,
    name: "Atlas",
    personaId: TEMPLATE_ID,
    personaVersion: PREVIOUS_TEMPLATE_VERSION,
    status: "running" as const,
    channelNames: ["agents"],
    backend: { type: "local" as const },
  },
  {
    pubkey: TEST_IDENTITIES.bob.pubkey,
    name: "Beacon",
    personaId: TEMPLATE_ID,
    personaVersion: PREVIOUS_TEMPLATE_VERSION,
    status: "stopped" as const,
    channelNames: ["agents"],
    backend: { type: "local" as const },
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
  },
  {
    pubkey: TEST_IDENTITIES.outsider.pubkey,
    name: "Drift",
    personaId: TEMPLATE_ID,
    personaVersion: PREVIOUS_TEMPLATE_VERSION,
    status: "stopped" as const,
    channelNames: ["agents"],
    backend: { type: "local" as const },
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
      personas: [
        {
          id: TEMPLATE_ID,
          displayName: TEMPLATE_NAME,
          systemPrompt:
            "Analyze campaign performance and explain what changed.",
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

    await page
      .locator("#persona-system-prompt")
      .fill(
        "Analyze campaign performance, explain what changed, and recommend the next action.",
      );
    const saveButton = templateEditor.getByTestId("persona-dialog-submit");
    await expect(saveButton).toBeEnabled({ timeout: 10_000 });
    const commandsBeforeSave = await page.evaluate(
      () => window.__BUZZ_E2E_COMMAND_LOG__?.length ?? 0,
    );
    await saveButton.click();
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
    await capture(
      page,
      updateReview,
      "03-update-selection-review-after-save.png",
    );

    await updateButton.click();
    await expect(
      updateReview.getByTestId("template-rollout-progress"),
    ).toBeVisible({ timeout: 2_000 });
    // The mock rollout settles after 700 ms. A short animation ceiling lets
    // the progress surface paint without waiting through the entire rollout.
    await capture(page, updateReview, "04-pending-update-progress.png", 75);

    await expect(
      updateReview.getByText("Agents updated", { exact: true }),
    ).toBeVisible({ timeout: 10_000 });
    await expect(updateReview).toContainText("3 agents were updated.");
    await expect(updateReview).toContainText("Updated and ready");
    await expect(updateReview).toContainText(
      "Updated · starts on this version next time",
    );
    await capture(page, updateReview, "05-completed-update.png");
  });
});
