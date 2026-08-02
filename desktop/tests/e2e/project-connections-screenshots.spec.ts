import { expect, test, type Locator, type Page } from "@playwright/test";

import { waitForAnimations } from "../helpers/animations";
import { installMockBridge, TEST_IDENTITIES } from "../helpers/bridge";

const SHOTS = "test-results/project-connections-screenshots";
const PROJECT_CHANNEL_ID = "9a1657ac-f7aa-5db0-b632-d8bbeb6dfb50";
const CONNECTION_ID = "connection-google-analytics";
const DEFAULT_MOCK_PUBKEY = "deadbeef".repeat(8);
const PROJECT_SCOPE = {
  relayUrl: "ws://localhost:3000",
  operatorPubkey: DEFAULT_MOCK_PUBKEY,
  repoAddress: `30617:${DEFAULT_MOCK_PUBKEY}:buzz`,
  channelId: PROJECT_CHANNEL_ID,
};

async function capture(page: Page, subject: Locator, filename: string) {
  await waitForAnimations(page);
  await subject.screenshot({ path: `${SHOTS}/${filename}` });
}

async function openConnections(page: Page) {
  await page.goto("/", { waitUntil: "domcontentloaded" });
  await page.getByTestId("open-projects-view").click();
  await page.getByRole("button", { name: "Repositories", exact: true }).click();
  const project = page
    .locator(
      '[data-testid="project-card-buzz"], [data-testid="project-row-buzz"]',
    )
    .first();
  await expect(project).toBeVisible({ timeout: 10_000 });
  await project.click();
  await page.getByRole("tab", { name: "Connections", exact: true }).click();
  await expect(page.getByTestId("project-connections-panel")).toBeVisible();
}

test.describe("Project Connections screenshots", () => {
  test.use({ viewport: { width: 1280, height: 900 } });

  test.beforeEach(async ({ page }) => {
    await page.addInitScript(() => {
      window.localStorage.setItem(
        "buzz-feature-overrides-v1",
        JSON.stringify({ projects: true }),
      );
    });
    await installMockBridge(page, {
      projectChannelId: PROJECT_CHANNEL_ID,
      projectConnections: [
        {
          id: CONNECTION_ID,
          projectScope: PROJECT_SCOPE,
          name: "Google Analytics",
          provider: "Google Analytics",
          capabilityIds: ["mcp.tool.run_report", "mcp.tool.export_report"],
          discoveredTools: ["run_report", "export_report"],
          command: "analytics-connector",
          args: ["--account", "acme"],
          envKeys: ["GOOGLE_ANALYTICS_TOKEN"],
          health: {
            status: "ready",
            lastVerifiedAt: "2026-08-02T14:30:00.000Z",
            detail: "2 tools available",
          },
          createdAt: "2026-08-02T14:00:00.000Z",
          updatedAt: "2026-08-02T14:30:00.000Z",
          generation: 2,
        },
      ],
      managedAgents: [
        {
          pubkey: TEST_IDENTITIES.alice.pubkey,
          name: "Campaign Analyst",
          status: "running",
          projectScope: PROJECT_SCOPE,
          toolRequirements: [
            {
              id: "analytics",
              label: "Analytics reports",
              capability: "mcp.tool.run_report",
              required: true,
            },
          ],
          connectionBindings: { analytics: CONNECTION_ID },
        },
      ],
    });
  });

  test("shows ready connections, trusted setup, and safe removal impact", async ({
    page,
  }) => {
    await openConnections(page);

    const panel = page.getByTestId("project-connections-panel");
    await expect(panel).toContainText("Google Analytics");
    await expect(panel).toContainText("Ready");
    await expect(panel).toContainText("Run Report");
    await capture(page, panel, "01-ready-connection.png");

    await panel.getByRole("button", { name: "Edit Google Analytics" }).click();
    const edit = page.getByRole("dialog", { name: "Edit connection" });
    await expect(edit).toContainText(
      "Buzz tests your changes before saving and restarts affected agents when needed.",
    );
    await edit.getByRole("button", { name: "Cancel" }).click();

    await panel.getByRole("button", { name: "Add connection" }).click();
    const setup = page.getByRole("dialog", { name: "Add connection" });
    await setup.getByLabel("Connection name").fill("Issue tracker");
    await setup.getByLabel("Service").fill("Linear");
    await setup.getByLabel("Connection command").fill("linear-connector");
    await setup.getByRole("button", { name: "Technical details" }).click();
    await setup.getByLabel("Arguments").fill("--workspace\nacme");
    await setup.getByLabel("Secret 1 name").fill("LINEAR_API_TOKEN");
    await setup
      .getByLabel(
        "I trust this program to run on my computer. It can use the secrets entered here and anything those credentials can access.",
      )
      .check();
    await capture(page, setup, "02-add-connection.png");
    await setup.getByRole("button", { name: "Cancel" }).click();

    await panel
      .getByRole("button", { name: "Remove Google Analytics" })
      .click();
    const impact = page.getByRole("alertdialog", {
      name: "Connection is still in use",
    });
    await expect(impact).toContainText("Campaign Analyst");
    await expect(
      impact.getByRole("button", { name: "Remove connection" }),
    ).toHaveCount(0);
    await capture(page, impact, "03-removal-impact.png");
  });
});
