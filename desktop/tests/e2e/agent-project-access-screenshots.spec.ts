import { expect, test, type Page } from "@playwright/test";

import { waitForAnimations } from "../helpers/animations";
import { installMockBridge, TEST_IDENTITIES } from "../helpers/bridge";

const SHOTS = "test-results/agent-project-access-screenshots";
const PROJECT_CHANNEL_ID = "9a1657ac-f7aa-5db0-b632-d8bbeb6dfb50";
const DEFAULT_MOCK_PUBKEY = "deadbeef".repeat(8);
const AGENT_PUBKEY = TEST_IDENTITIES.tyler.pubkey;
const AGENT_NAME = "Campaign Analyst";
const ANALYTICS_REQUIREMENT = {
  id: "analytics",
  label: "Analytics reports",
  capability: "mcp.tool.run_report",
  required: true,
};
const PROJECT_SCOPE = {
  relayUrl: "ws://localhost:3000",
  operatorPubkey: DEFAULT_MOCK_PUBKEY,
  projectAddress: `30617:${DEFAULT_MOCK_PUBKEY}:buzz`,
  channelId: PROJECT_CHANNEL_ID,
};
const LEGACY_PROJECT_SCOPE = {
  relayUrl: PROJECT_SCOPE.relayUrl,
  operatorPubkey: PROJECT_SCOPE.operatorPubkey,
  repoAddress: PROJECT_SCOPE.projectAddress,
  channelId: PROJECT_SCOPE.channelId,
};
const CONNECTION_ID = "connection-google-analytics";

async function openAgentEditor(page: Page) {
  await page.goto("/", { waitUntil: "domcontentloaded" });
  await page.getByTestId("open-agents-view").click();
  await page
    .getByRole("button", { name: `${AGENT_NAME} agent profile` })
    .click();
  await page.getByTestId("user-profile-edit-agent").click();
  await expect(
    page.getByRole("dialog", { name: `Edit ${AGENT_NAME}` }),
  ).toBeVisible({ timeout: 10_000 });
  await expect(page.locator("#edit-agent-llm-provider")).toBeVisible({
    timeout: 10_000,
  });
}

async function pickDropdownOption(
  page: Page,
  triggerId: string,
  optionName: string,
) {
  await page.locator(`#${triggerId}`).click();
  await page.getByRole("menuitemradio", { name: optionName }).click();
}

test.describe("agent Project access flow", () => {
  test.use({ viewport: { width: 1280, height: 900 } });

  test("assigns a Project and connection to an existing agent", async ({
    page,
  }) => {
    await installMockBridge(page, {
      projectChannelId: PROJECT_CHANNEL_ID,
      projectConnections: [
        {
          id: CONNECTION_ID,
          projectScope: PROJECT_SCOPE,
          name: "Google Analytics",
          provider: "Google Analytics",
          capabilityIds: [ANALYTICS_REQUIREMENT.capability],
          discoveredTools: ["run_report"],
          command: "analytics-connector",
          args: [],
          envKeys: ["GOOGLE_ANALYTICS_TOKEN"],
          health: {
            status: "ready",
            lastVerifiedAt: "2026-08-04T12:00:00.000Z",
            detail: "1 tool available",
          },
          createdAt: "2026-08-04T11:00:00.000Z",
          updatedAt: "2026-08-04T12:00:00.000Z",
          generation: 1,
        },
      ],
      managedAgents: [
        {
          pubkey: AGENT_PUBKEY,
          name: AGENT_NAME,
          status: "stopped",
          channelNames: ["agents"],
          toolRequirements: [ANALYTICS_REQUIREMENT],
        },
      ],
    });

    await openAgentEditor(page);
    await pickDropdownOption(page, "agent-project", "buzz");
    await pickDropdownOption(
      page,
      "agent-tool-binding-analytics",
      "Google Analytics",
    );

    const dialog = page.getByTestId("edit-agent-dialog");
    await page
      .getByTestId("agent-project-access-section")
      .scrollIntoViewIfNeeded();
    await waitForAnimations(page);
    await dialog.screenshot({ path: `${SHOTS}/01-project-and-connection.png` });

    const submit = page.getByTestId("edit-agent-dialog-submit");
    await expect(submit).toBeEnabled();
    await submit.click();
    await expect(dialog).not.toBeVisible();

    const updateInput = await page.evaluate(() => {
      const command = [...(window.__BUZZ_E2E_COMMAND_LOG__ ?? [])]
        .reverse()
        .find((entry) => entry.command === "update_managed_agent");
      return command?.payload?.input;
    });
    expect(updateInput).toMatchObject({
      pubkey: AGENT_PUBKEY,
      projectScope: PROJECT_SCOPE,
      connectionBindings: { analytics: CONNECTION_ID },
    });
  });

  test("manages missing connections without losing the agent draft", async ({
    page,
  }) => {
    await installMockBridge(page, {
      projectChannelId: PROJECT_CHANNEL_ID,
      projectConnections: [],
      managedAgents: [
        {
          pubkey: AGENT_PUBKEY,
          name: AGENT_NAME,
          status: "stopped",
          channelNames: ["agents"],
          toolRequirements: [ANALYTICS_REQUIREMENT],
        },
      ],
    });

    await openAgentEditor(page);
    await page.locator("#edit-agent-name").fill("Lifecycle Analyst");
    await pickDropdownOption(page, "agent-project", "buzz");
    await page.getByRole("button", { name: "Manage connections" }).click();

    const connections = page.getByRole("dialog", {
      name: "Connections for buzz",
    });
    await expect(connections).toBeVisible();
    await expect(connections).toContainText(
      "Add or test a connection without losing this agent setup.",
    );
    await expect(connections).toContainText("No connections yet");
    await waitForAnimations(page);
    await connections.screenshot({
      path: `${SHOTS}/02-manage-connections-in-flow.png`,
    });

    await connections.getByRole("button", { name: "Back to agent" }).click();
    await expect(connections).not.toBeVisible();
    await expect(page.locator("#edit-agent-name")).toHaveValue(
      "Lifecycle Analyst",
    );
    await expect(
      page.getByTestId("agent-project-access-section"),
    ).toContainText("buzz");
  });

  test("does not rewrite an unchanged legacy Project scope", async ({
    page,
  }) => {
    await installMockBridge(page, {
      projectChannelId: PROJECT_CHANNEL_ID,
      projectConnections: [
        {
          id: CONNECTION_ID,
          projectScope: PROJECT_SCOPE,
          name: "Google Analytics",
          provider: "Google Analytics",
          capabilityIds: [ANALYTICS_REQUIREMENT.capability],
          discoveredTools: ["run_report"],
          command: "analytics-connector",
          args: [],
          envKeys: ["GOOGLE_ANALYTICS_TOKEN"],
          health: {
            status: "ready",
            lastVerifiedAt: "2026-08-04T12:00:00.000Z",
            detail: "1 tool available",
          },
          createdAt: "2026-08-04T11:00:00.000Z",
          updatedAt: "2026-08-04T12:00:00.000Z",
          generation: 1,
        },
      ],
      managedAgents: [
        {
          pubkey: AGENT_PUBKEY,
          name: AGENT_NAME,
          status: "stopped",
          channelNames: ["agents"],
          projectScope: LEGACY_PROJECT_SCOPE,
          toolRequirements: [ANALYTICS_REQUIREMENT],
          connectionBindings: { analytics: CONNECTION_ID },
        },
      ],
    });

    await openAgentEditor(page);
    await page.locator("#edit-agent-name").fill("Lifecycle Analyst");
    const submit = page.getByTestId("edit-agent-dialog-submit");
    await expect(submit).toBeEnabled();
    await submit.click();

    const updateInput = await page.evaluate(() => {
      const command = [...(window.__BUZZ_E2E_COMMAND_LOG__ ?? [])]
        .reverse()
        .find((entry) => entry.command === "update_managed_agent");
      return command?.payload?.input;
    });
    expect(updateInput).toMatchObject({
      pubkey: AGENT_PUBKEY,
      name: "Lifecycle Analyst",
    });
    expect(updateInput?.projectScope).toBeUndefined();
    expect(updateInput?.connectionBindings).toBeUndefined();
  });
});
