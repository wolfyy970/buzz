import { expect, test, type Locator, type Page } from "@playwright/test";

import { waitForAnimations } from "../helpers/animations";
import { installMockBridge } from "../helpers/bridge";
import type { ProjectConnection } from "../../src/shared/api/tauriProjectConnections";

const SHOTS = "test-results/project-connections-screenshots";
const CONNECTION_ID = "connection-google-analytics";
const DEFAULT_MOCK_PUBKEY = "deadbeef".repeat(8);
const PROJECT_SCOPE = {
  relayUrl: "ws://localhost:3000",
  operatorPubkey: DEFAULT_MOCK_PUBKEY,
  projectAddress: `30621:${DEFAULT_MOCK_PUBKEY}:buzz`,
};

function connectionFixture(
  overrides: Partial<ProjectConnection> = {},
): ProjectConnection {
  return {
    id: CONNECTION_ID,
    projectScope: PROJECT_SCOPE,
    name: "Google Analytics",
    provider: "Google Analytics",
    capabilityIds: [
      `mcp.tool.${CONNECTION_ID}.run_report`,
      `mcp.tool.${CONNECTION_ID}.export_report`,
    ],
    discoveredTools: ["run_report", "export_report"],
    command: "/opt/homebrew/bin/analytics-connector",
    args: ["--account", "acme"],
    envKeys: ["GOOGLE_ANALYTICS_TOKEN"],
    health: {
      status: "ready",
      lastVerifiedAt: "2026-08-02T14:30:00.000Z",
      detail: null,
    },
    createdAt: "2026-08-02T14:00:00.000Z",
    updatedAt: "2026-08-02T14:30:00.000Z",
    ...overrides,
  };
}

async function capture(page: Page, subject: Locator, filename: string) {
  await waitForAnimations(page);
  await subject.screenshot({ path: `${SHOTS}/${filename}` });
}

async function captureVisible(page: Page, subject: Locator, filename: string) {
  await waitForAnimations(page);
  const box = await subject.boundingBox();
  const viewport = page.viewportSize();
  if (!box || !viewport) {
    throw new Error("Cannot capture a subject outside the current viewport");
  }

  const x = Math.max(0, box.x);
  const y = Math.max(0, box.y);
  const right = Math.min(viewport.width, box.x + box.width);
  const bottom = Math.min(viewport.height, box.y + box.height);
  if (right <= x || bottom <= y) {
    throw new Error("Cannot capture a subject outside the current viewport");
  }

  await page.screenshot({
    path: `${SHOTS}/${filename}`,
    clip: { x, y, width: right - x, height: bottom - y },
  });
}

async function scrollDialogToTop(dialog: Locator) {
  await dialog.evaluate((element) => {
    element.parentElement?.scrollTo({ top: 0 });
  });
}

async function openProject(page: Page, projectSlug = "buzz") {
  await page.goto("/", { waitUntil: "domcontentloaded" });
  await page.getByTestId("open-projects-view").click();
  await page.getByTestId("projects-section-projects").click();
  const project = page
    .locator(
      `[data-testid="project-card-${projectSlug}"], [data-testid="project-row-${projectSlug}"]`,
    )
    .first();
  await expect(project).toBeVisible({ timeout: 10_000 });
  await project.click();
}

async function openConnections(page: Page) {
  await openProject(page);
  await page.getByRole("tab", { name: "Connections", exact: true }).click();
  await expect(page.getByTestId("project-connections-panel")).toBeVisible();
}

async function switchToEmptyProject(page: Page, force = false) {
  if (force) {
    await page
      .getByTestId("open-projects-view")
      .evaluate((element: HTMLElement) => element.click());
    await expect(
      page.getByRole("heading", { name: "Projects", exact: true }),
    ).toBeVisible();
    await page
      .getByRole("button", { name: "View Empty Project", exact: true })
      .click();
    const panel = page.getByTestId("project-connections-panel");
    await expect(panel).toContainText("No connections yet");
    return panel;
  }
  await page.getByTestId("open-projects-view").click({ force });
  await page.getByTestId("projects-section-projects").click({ force });
  await page
    .locator(
      '[data-testid="project-card-empty"], [data-testid="project-row-empty"]',
    )
    .first()
    .click({ force });
  const panel = page.getByTestId("project-connections-panel");
  await expect(panel).toContainText("No connections yet");
  return panel;
}

async function switchToRepositoryProject(page: Page, projectSlug = "buzz") {
  await page.getByTestId("open-projects-view").click();
  await page.getByTestId("projects-section-projects").click();
  await page
    .locator(
      `[data-testid="project-card-${projectSlug}"], [data-testid="project-row-${projectSlug}"]`,
    )
    .first()
    .click();
  await page.getByRole("tab", { name: "Connections", exact: true }).click();
  const panel = page.getByTestId("project-connections-panel");
  await expect(panel).toBeVisible();
  return panel;
}

test.describe("Project Connections screenshots", () => {
  test.use({ viewport: { width: 1280, height: 900 } });

  test.beforeEach(async ({ page }, testInfo) => {
    const zeroRepositoryProject = testInfo.title.includes("zero repositories");
    const seedZeroRepositoryProject =
      zeroRepositoryProject || testInfo.title.includes("scope switch");
    await page.addInitScript(
      ({ identityPubkey, seedZeroRepositoryProject }) => {
        window.localStorage.setItem(
          "buzz-feature-overrides-v1",
          JSON.stringify({ projects: true }),
        );
        if (seedZeroRepositoryProject) {
          window.__BUZZ_E2E_EXTRA_PROJECT_EVENTS__ = [
            {
              id: "project-empty".padEnd(64, "0"),
              kind: 30621,
              pubkey: identityPubkey,
              created_at: 1_800_000_000,
              content: "",
              tags: [
                ["d", "empty"],
                ["name", "Empty Project"],
                ["description", "A Project before its first repository."],
              ],
            },
          ];
        }
      },
      {
        identityPubkey: DEFAULT_MOCK_PUBKEY,
        seedZeroRepositoryProject,
      },
    );
    const staleApproval = testInfo.title.includes("stale approval");
    const multipleRows = testInfo.title.includes("only the tested row");
    const manyTools = testInfo.title.includes("all discovered tools");
    const exactWorkspaceCleanup = testInfo.title.includes(
      "exact workspace cleanup",
    );
    const primaryConnection = connectionFixture(
      staleApproval
        ? {
            health: {
              status: "approval_required",
              lastVerifiedAt: null,
              detail: null,
            },
          }
        : manyTools
          ? {
              capabilityIds: Array.from(
                { length: 6 },
                (_, index) => `mcp.tool.${CONNECTION_ID}.tool_${index + 1}`,
              ),
              discoveredTools: Array.from(
                { length: 6 },
                (_, index) => `tool_${index + 1}`,
              ),
            }
          : undefined,
    );
    await installMockBridge(page, {
      projectConnectionDeleteError: testInfo.title.includes("delete failure")
        ? "Keyring unavailable."
        : undefined,
      projectConnectionBulkDeleteFailures: testInfo.title.includes(
        "partial deletion recovery",
      )
        ? 1
        : undefined,
      projectConnectionSaveDelayMs: testInfo.title.includes("pending save")
        ? 500
        : undefined,
      projectConnectionTestDelayMs: multipleRows ? 500 : undefined,
      projectConnectionTestError: testInfo.title.includes("test failure")
        ? "Server exited."
        : undefined,
      projectConnections: [
        ...(zeroRepositoryProject ? [] : [primaryConnection]),
        ...(multipleRows
          ? [
              connectionFixture({
                id: "connection-linear",
                name: "Linear",
                provider: "Linear",
                capabilityIds: ["mcp.tool.connection-linear.search"],
                discoveredTools: ["search"],
                command: "/opt/homebrew/bin/linear-connector",
                envKeys: ["LINEAR_API_TOKEN"],
              }),
            ]
          : []),
        ...(exactWorkspaceCleanup
          ? [
              connectionFixture({
                id: "connection-same-project-workspace-b",
                name: "Workspace B analytics",
                projectScope: {
                  ...PROJECT_SCOPE,
                  relayUrl: "wss://relay-b.example",
                },
              }),
            ]
          : []),
      ],
    });
  });

  test("exact workspace cleanup preserves the same Project in workspace B", async ({
    page,
  }) => {
    await page.goto("/", { waitUntil: "domcontentloaded" });
    const counts = await page.evaluate(async () => {
      const invoke = window.__BUZZ_E2E_INVOKE_MOCK_COMMAND__;
      if (!invoke) throw new Error("mock command bridge unavailable");
      await invoke("delete_project_connections_for_project", {
        projectScope: {
          relayUrl: "ws://localhost:3000",
          operatorPubkey: "deadbeef".repeat(8),
          projectAddress: `30621:${"deadbeef".repeat(8)}:buzz`,
        },
      });
      const workspaceA = (await invoke("list_project_connections", {
        projectScope: {
          relayUrl: "ws://localhost:3000",
          operatorPubkey: "deadbeef".repeat(8),
          projectAddress: `30621:${"deadbeef".repeat(8)}:buzz`,
        },
      })) as unknown[];
      const workspaceB = (await invoke("list_project_connections", {
        projectScope: {
          relayUrl: "wss://relay-b.example",
          operatorPubkey: "deadbeef".repeat(8),
          projectAddress: `30621:${"deadbeef".repeat(8)}:buzz`,
        },
      })) as unknown[];
      return { workspaceA: workspaceA.length, workspaceB: workspaceB.length };
    });
    expect(counts).toEqual({ workspaceA: 0, workspaceB: 1 });
  });

  test("partial deletion recovery persists and retries local cleanup", async ({
    page,
  }) => {
    await page.goto("/", { waitUntil: "domcontentloaded" });
    await page.getByTestId("open-projects-view").click();
    await page.getByTestId("projects-section-projects").click();
    await page.getByRole("button", { name: "More options for buzz" }).click();
    await page.getByRole("menuitem", { name: "Delete project" }).click();
    await page.getByTestId("project-delete-confirm-button-buzz").click();

    await expect
      .poll(() =>
        page.evaluate(
          () =>
            window.__BUZZ_E2E_SIGNED_EVENTS__?.filter(
              (event) => event.kind === 5,
            ).length ?? 0,
        ),
      )
      .toBe(1);
    await expect
      .poll(() =>
        page.evaluate(
          () =>
            window.__BUZZ_E2E_PROJECT_CONNECTION_BULK_DELETE_ATTEMPTS__ ?? 0,
        ),
      )
      .toBe(1);
    await expect
      .poll(() =>
        page.evaluate(
          () =>
            JSON.parse(
              window.localStorage.getItem(
                "buzz.projects.pending-connection-cleanup.v1",
              ) ?? "[]",
            ).length,
        ),
      )
      .toBe(1);
    const pending = await page.evaluate(() =>
      JSON.parse(
        window.localStorage.getItem(
          "buzz.projects.pending-connection-cleanup.v1",
        ) ?? "[]",
      ),
    );
    expect(pending).toHaveLength(1);
    expect(pending[0].state).toBe("published");

    const retry = page.getByRole("button", { name: "Retry cleanup" });
    await expect(retry).toBeVisible();
    await retry.click();
    await expect(
      page.getByText("Local connection cleanup finished."),
    ).toBeVisible();
    await expect
      .poll(() =>
        page.evaluate(
          () =>
            JSON.parse(
              window.localStorage.getItem(
                "buzz.projects.pending-connection-cleanup.v1",
              ) ?? "[]",
            ).length,
        ),
      )
      .toBe(0);
  });

  test("names the exact Project and community scope accessibly", async ({
    page,
  }) => {
    await openConnections(page);

    const panel = page.getByTestId("project-connections-panel");
    await panel.getByRole("button", { name: "Add connection" }).click();
    const addDialog = page.getByRole("dialog", {
      name: "Add Project connection to Buzz",
    });
    await expect(addDialog).toHaveAccessibleDescription(
      /buzz in E2E Test at ws:\/\/localhost:3000.*credentials stay on this device/,
    );
    const addScope = addDialog.getByRole("group", {
      name: "Connection scope",
    });
    await expect(addScope).toContainText("Project");
    await expect(addScope.getByText("buzz", { exact: true })).toBeVisible();
    await expect(addScope).toContainText("Community");
    await expect(addScope).toContainText("E2E Test");
    await expect(addScope).toContainText("Relay");
    await expect(addScope).toContainText("ws://localhost:3000");
    await expect(addScope).toContainText("Credentials");
    await expect(addScope).toContainText("Stay on this device");
    await addDialog.getByRole("button", { name: "Cancel" }).click();

    await panel.getByRole("button", { name: "Edit Google Analytics" }).click();
    const editDialog = page.getByRole("dialog", {
      name: "Edit Google Analytics for Buzz",
    });
    await expect(editDialog).toHaveAccessibleDescription(
      /Edit this Project connection in E2E Test at ws:\/\/localhost:3000.*credentials stay on this device/,
    );
    await expect(
      editDialog
        .getByRole("group", { name: "Connection scope" })
        .getByText("buzz", { exact: true }),
    ).toBeVisible();
  });

  test("covers setup, verification, and removal", async ({ page }) => {
    await openConnections(page);

    const panel = page.getByTestId("project-connections-panel");
    await expect(panel).toContainText("Google Analytics");
    await expect(panel).toContainText("Tools found");
    await expect(panel).toContainText("Run Report");
    await capture(page, panel, "01-ready-connection.png");

    const repositoryPicker = page.getByTestId("project-repository-picker");
    await expect(repositoryPicker).not.toBeVisible();
    await page
      .getByRole("navigation", { name: "Project breadcrumb" })
      .getByRole("button", { name: "Buzz" })
      .click();
    await expect(
      page.getByRole("tab", { name: "Overview", exact: true }),
    ).toHaveAttribute("data-state", "active");
    await expect(repositoryPicker).toBeVisible();
    await repositoryPicker.click();
    await page.getByTestId("project-repository-relay-tools").click();
    await expect(repositoryPicker).toContainText("relay-tools");
    await page.getByRole("tab", { name: "Connections", exact: true }).click();
    await expect(page.getByTestId("project-connections-panel")).toContainText(
      "Google Analytics",
    );

    await panel.getByRole("button", { name: "Add connection" }).click();
    const setup = page.getByRole("dialog", {
      name: "Add Project connection to Buzz",
    });
    await setup.getByLabel("Connection name").fill("Issue tracker");
    await setup.getByLabel("Service").fill("Linear");
    await setup
      .getByLabel("Connection command")
      .fill("/opt/homebrew/bin/linear-connector");
    await setup.getByRole("button", { name: "Technical details" }).click();
    await setup
      .getByRole("textbox", { name: "Arguments", exact: true })
      .fill("--workspace\nacme");
    await setup.getByLabel("Secret 1 name").fill("LINEAR_API_TOKEN");
    await setup.getByLabel("Secret 1 value").fill("not-shown-in-capture");
    await setup
      .getByLabel(/I trust this executable and the arguments above/)
      .check();
    await scrollDialogToTop(setup);
    await expect(
      setup.getByRole("heading", {
        name: "Add Project connection to Buzz",
      }),
    ).toBeInViewport();
    await captureVisible(page, setup, "02-add-connection.png");
    await setup.getByRole("button", { name: "Save and test" }).click();
    const issueTrackerRow = panel
      .locator('[data-testid^="project-connection-"]')
      .filter({ hasText: "Issue tracker" });
    await expect(issueTrackerRow).toContainText("Tools found");
    await expect(issueTrackerRow).toContainText("Linear.Search Issues");

    const analyticsRow = panel.getByTestId(
      `project-connection-${CONNECTION_ID}`,
    );
    await analyticsRow
      .getByRole("button", { name: "Edit Google Analytics" })
      .click();
    const edit = page.getByRole("dialog", {
      name: "Edit Google Analytics for Buzz",
    });
    await edit
      .getByLabel("Connection command")
      .fill("/opt/homebrew/bin/analytics-connector-v2");
    await edit
      .getByLabel(/I trust this executable and the arguments above/)
      .check();
    await edit.getByRole("button", { name: "Save and test" }).click();
    await expect(analyticsRow).toContainText("Tools found");
    await expect(analyticsRow).toContainText("Analytics.Weekly Summary");

    await panel
      .getByRole("button", { name: "Remove Google Analytics" })
      .click();
    const confirmation = page.getByRole("alertdialog", {
      name: "Remove Google Analytics?",
    });
    await capture(page, confirmation, "03-remove-connection-confirmation.png");
    await confirmation
      .getByRole("button", { name: "Remove connection" })
      .click();
    await expect(panel).not.toContainText("Google Analytics");
    await expect(panel).toContainText("Issue tracker");
  });

  test("stale approval can be reviewed without changing the command", async ({
    page,
  }) => {
    await openConnections(page);

    const panel = page.getByTestId("project-connections-panel");
    await expect(
      panel.getByRole("button", { name: "Review command Google Analytics" }),
    ).toBeVisible();
    await panel
      .getByRole("button", { name: "Review command Google Analytics" })
      .click();
    const dialog = page.getByRole("dialog", {
      name: "Edit Google Analytics for Buzz",
    });
    await expect(
      dialog.getByLabel(/I trust this executable and the arguments above/),
    ).toBeVisible();
    await dialog
      .getByLabel(/I trust this executable and the arguments above/)
      .check();
    await dialog.getByRole("button", { name: "Save and test" }).click();

    await expect(panel).toContainText("Tools found");
    await expect(panel).toContainText("Analytics.Weekly Summary");
  });

  test("only the tested row reports progress", async ({ page }) => {
    await openConnections(page);

    const panel = page.getByTestId("project-connections-panel");
    await panel
      .getByRole("button", { name: "Test again Google Analytics" })
      .click();

    await expect(panel.getByText("Testing Google Analytics…")).toBeVisible();
    await expect(
      panel.getByRole("button", { name: "Testing Google Analytics" }),
    ).toBeDisabled();
    await expect(
      panel.getByRole("button", { name: "Test again Linear" }),
    ).toBeDisabled();
    await expect(panel.getByText("Testing Linear…")).toHaveCount(0);
    await expect(panel.getByText("Testing Google Analytics…")).toHaveCount(0, {
      timeout: 2_000,
    });
  });

  test("all discovered tools can be inspected", async ({ page }) => {
    await openConnections(page);

    const row = page.getByTestId(`project-connection-${CONNECTION_ID}`);
    await expect(row).toContainText("Tool 1");
    await expect(row).not.toContainText("Tool 6");
    await row.getByRole("button", { name: "Show 2 more" }).click();
    await expect(row).toContainText("Tool 6");
    await expect(
      row.getByRole("button", { name: "Show fewer" }),
    ).toHaveAttribute("aria-expanded", "true");
    await row.getByRole("button", { name: "Show fewer" }).click();
    await expect(row).not.toContainText("Tool 6");
  });

  test("delete failure stays in the confirmation flow", async ({ page }) => {
    await openConnections(page);

    const panel = page.getByTestId("project-connections-panel");
    await panel
      .getByRole("button", { name: "Remove Google Analytics" })
      .click();
    const confirmation = page.getByRole("alertdialog", {
      name: "Remove Google Analytics?",
    });
    await confirmation
      .getByRole("button", { name: "Remove connection" })
      .click();

    await expect(confirmation).toBeVisible();
    await expect(confirmation).toContainText(
      "Couldn't remove Google Analytics: Keyring unavailable.",
    );
    await expect(panel).toContainText("Google Analytics");
  });

  test("remains usable at maximum text zoom", async ({ page }) => {
    await page.setViewportSize({ width: 800, height: 700 });
    await openProject(page);
    await page.evaluate(() => {
      document.documentElement.style.fontSize = "24px";
      window.localStorage.setItem("buzz:text-scale", "1.5");
    });
    const connectionsTab = page.getByRole("tab", {
      name: "Connections",
      exact: true,
    });
    await expect(connectionsTab).toBeInViewport();
    await connectionsTab.click();

    const panel = page.getByTestId("project-connections-panel");
    await expect(panel).toContainText("Google Analytics");
    await expect
      .poll(() =>
        panel.evaluate((element) => element.scrollWidth <= element.clientWidth),
      )
      .toBe(true);
    await capture(page, panel, "04-maximum-text-zoom.png");
  });

  test("create flow remains operable at maximum text zoom", async ({
    page,
  }) => {
    await page.setViewportSize({ width: 800, height: 700 });
    await openConnections(page);
    await page.evaluate(() => {
      document.documentElement.style.fontSize = "24px";
      window.localStorage.setItem("buzz:text-scale", "1.5");
    });
    await page
      .getByTestId("project-connections-panel")
      .getByRole("button", { name: "Add connection" })
      .click();

    const dialog = page.getByRole("dialog", {
      name: "Add Project connection to Buzz",
    });
    await expect
      .poll(() =>
        dialog.evaluate(
          (element) => element.scrollWidth <= element.clientWidth,
        ),
      )
      .toBe(true);
    await dialog.getByLabel("Connection name").fill("Linear");
    await dialog.getByLabel("Service").fill("Linear");
    await dialog.getByLabel("Connection command").fill("/usr/bin/true");
    await dialog.getByRole("button", { name: "Technical details" }).click();
    await dialog.getByLabel("Secret 1 name").fill("LINEAR_API_TOKEN");
    await dialog.getByLabel("Secret 1 value").fill("test-only");
    await dialog
      .getByLabel(/I trust this executable and the arguments above/)
      .check();
    await dialog
      .getByRole("button", { name: "Save and test" })
      .scrollIntoViewIfNeeded();
    await expect(
      dialog.getByRole("button", { name: "Save and test" }),
    ).toBeInViewport();
    await captureVisible(page, dialog, "05-maximum-text-zoom-setup.png");

    await page.keyboard.press("Tab");
    await expect
      .poll(() =>
        dialog.evaluate((element) => element.contains(document.activeElement)),
      )
      .toBe(true);
    await dialog.getByRole("button", { name: "Save and test" }).click();
    const created = page
      .locator('[data-testid^="project-connection-"]')
      .filter({ hasText: "Linear" });
    await expect(created).toContainText("Tools found");
  });

  test("pending save cannot be dismissed", async ({ page }) => {
    await openConnections(page);

    await page
      .getByTestId("project-connections-panel")
      .getByRole("button", { name: "Add connection" })
      .click();
    const dialog = page.getByRole("dialog", {
      name: "Add Project connection to Buzz",
    });
    await dialog.getByLabel("Connection name").fill("Linear");
    await dialog.getByLabel("Service").fill("Linear");
    await dialog
      .getByLabel("Connection command")
      .fill("/opt/homebrew/bin/linear-connector");
    await dialog
      .getByLabel(/I trust this executable and the arguments above/)
      .check();
    await dialog.getByRole("button", { name: "Save and test" }).click();

    await expect(dialog.getByRole("button", { name: "Saving…" })).toBeVisible();
    await expect(dialog.getByRole("button", { name: "Close" })).toHaveCount(0);
    await page.keyboard.press("Escape");
    await expect(dialog).toBeVisible();
    await expect(dialog).toHaveCount(0, { timeout: 2_000 });
  });

  test("matches backend validation boundaries before saving", async ({
    page,
  }) => {
    await openConnections(page);

    await page
      .getByTestId("project-connections-panel")
      .getByRole("button", { name: "Add connection" })
      .click();
    const dialog = page.getByRole("dialog", {
      name: "Add Project connection to Buzz",
    });
    const submit = dialog.getByRole("button", { name: "Save and test" });
    await expect(submit).toBeDisabled();

    await dialog.getByLabel("Connection name").fill("x".repeat(128));
    await expect(dialog).not.toContainText(
      "Keep the connection name to 128 bytes or fewer.",
    );
    await dialog.getByLabel("Connection name").fill("😀".repeat(33));
    await expect(dialog).toContainText(
      "Keep the connection name to 128 bytes or fewer.",
    );
    await dialog.getByLabel("Connection name").fill("Analytics");

    await dialog.getByLabel("Service").fill("x".repeat(64));
    await expect(dialog).not.toContainText(
      "Keep the service name to 64 bytes or fewer.",
    );
    await dialog.getByLabel("Service").fill("é".repeat(33));
    await expect(dialog).toContainText(
      "Keep the service name to 64 bytes or fewer.",
    );
    await dialog.getByLabel("Service").fill("Google Analytics");

    await dialog.getByLabel("Connection command").fill(`/${"x".repeat(1_023)}`);
    await expect(dialog).not.toContainText(
      "Keep the command to 1024 bytes or fewer.",
    );
    await dialog.getByLabel("Connection command").fill(`/${"😀".repeat(256)}`);
    await expect(dialog).toContainText(
      "Keep the command to 1024 bytes or fewer.",
    );
    await dialog.getByLabel("Connection command").fill("/usr/bin/true");

    await dialog.getByRole("button", { name: "Technical details" }).click();
    await dialog.getByLabel("Secret 1 name").fill("BUZZ_PRIVATE_KEY");
    await dialog.getByLabel("Secret 1 value").fill("not-allowed");
    await expect(dialog).toContainText(
      "BUZZ_PRIVATE_KEY is managed by Buzz and cannot be used here.",
    );
    await expect(submit).toBeDisabled();
    await dialog.getByLabel("Secret 1 name").fill("");
    await dialog.getByLabel("Secret 1 value").fill("");

    const argumentsInput = dialog.getByRole("textbox", {
      name: "Arguments",
      exact: true,
    });
    await argumentsInput.fill("x".repeat(4_096));
    await expect(dialog).not.toContainText(
      "Use no more than 128 arguments, with each 4096 bytes or fewer.",
    );
    await argumentsInput.fill("😀".repeat(1_025));
    await expect(dialog).toContainText(
      "Use no more than 128 arguments, with each 4096 bytes or fewer.",
    );
    await argumentsInput.fill("--version");

    await expect(submit).toBeDisabled();
    await dialog
      .getByLabel(/I trust this executable and the arguments above/)
      .check();
    await expect(submit).toBeEnabled();
  });

  test("test failure leaves a recoverable row state", async ({ page }) => {
    await openConnections(page);

    const panel = page.getByTestId("project-connections-panel");
    await panel
      .getByRole("button", { name: "Test again Google Analytics" })
      .click();

    await expect(panel).toContainText("Unavailable");
    await expect(panel).toContainText(
      "The MCP server did not respond in time.",
    );
    await expect(
      panel.getByRole("button", { name: "Test again Google Analytics" }),
    ).toBeEnabled();
  });

  test("scope switch ignores a pending test from the previous Project", async ({
    page,
  }) => {
    await openConnections(page);
    await page.evaluate(() => {
      window.__BUZZ_E2E_DEFER_NEXT_PROJECT_CONNECTION_TEST__?.();
    });
    const panel = page.getByTestId("project-connections-panel");
    await panel
      .getByRole("button", { name: "Test again Google Analytics" })
      .click();
    await expect
      .poll(() =>
        page.evaluate(
          () => window.__BUZZ_E2E_PROJECT_CONNECTION_TEST_PENDING__ ?? 0,
        ),
      )
      .toBe(1);

    const emptyPanel = await switchToEmptyProject(page);
    expect(
      await page.evaluate(
        () => window.__BUZZ_E2E_RELEASE_PROJECT_CONNECTION_TEST__?.() ?? 0,
      ),
    ).toBe(1);
    await expect
      .poll(() =>
        page.evaluate(
          () => window.__BUZZ_E2E_PROJECT_CONNECTION_TEST_UI_SETTLED__ ?? 0,
        ),
      )
      .toBe(1);
    await expect(
      page.getByText("Tools found for Google Analytics."),
    ).toHaveCount(0);
    await expect(emptyPanel).not.toContainText("Google Analytics");
  });

  test("scope switch stops a pending save before its automatic test", async ({
    page,
  }) => {
    await openConnections(page);
    const panel = page.getByTestId("project-connections-panel");
    await panel.getByRole("button", { name: "Add connection" }).click();
    const dialog = page.getByRole("dialog", {
      name: "Add Project connection to Buzz",
    });
    await dialog.getByLabel("Connection name").fill("Issue tracker");
    await dialog.getByLabel("Service").fill("Linear");
    await dialog.getByLabel("Connection command").fill("/usr/bin/true");
    await dialog
      .getByLabel(/I trust this executable and the arguments above/)
      .check();
    await page.evaluate(() => {
      window.__BUZZ_E2E_DEFER_NEXT_PROJECT_CONNECTION_SAVE__?.();
    });
    await dialog.getByRole("button", { name: "Save and test" }).click();
    await expect
      .poll(() =>
        page.evaluate(
          () => window.__BUZZ_E2E_PROJECT_CONNECTION_SAVE_PENDING__ ?? 0,
        ),
      )
      .toBe(1);

    const emptyPanel = await switchToEmptyProject(page, true);
    expect(
      await page.evaluate(
        () => window.__BUZZ_E2E_RELEASE_PROJECT_CONNECTION_SAVE__?.() ?? 0,
      ),
    ).toBe(1);
    await expect
      .poll(() =>
        page.evaluate(
          () => window.__BUZZ_E2E_PROJECT_CONNECTION_SAVE_UI_SETTLED__ ?? 0,
        ),
      )
      .toBe(1);
    await expect
      .poll(() =>
        page.evaluate(
          () => window.__BUZZ_E2E_PROJECT_CONNECTION_TEST_UI_SETTLED__ ?? 0,
        ),
      )
      .toBe(0);
    await expect(page.getByText("Tools found for Issue tracker.")).toHaveCount(
      0,
    );
    await expect(emptyPanel).not.toContainText("Issue tracker");

    const restoredPanel = await switchToRepositoryProject(page);
    const issueTrackerRow = restoredPanel
      .locator('[data-testid^="project-connection-"]')
      .filter({ hasText: "Issue tracker" });
    await expect(issueTrackerRow).toContainText("Not tested");
    await expect(issueTrackerRow).toContainText("Automatic test interrupted");
    await expect(issueTrackerRow).toContainText(
      "Setup was saved, but its automatic test did not run.",
    );
    await expect(
      issueTrackerRow.getByRole("button", { name: "Test Issue tracker" }),
    ).toBeEnabled();
    await expect
      .poll(() =>
        page.evaluate(
          () =>
            JSON.parse(
              window.localStorage.getItem(
                "buzz.projects.pending-connection-automatic-tests.v1",
              ) ?? "[]",
            ).length,
        ),
      )
      .toBe(1);

    await issueTrackerRow
      .getByRole("button", { name: "Test Issue tracker" })
      .click();
    await expect(issueTrackerRow).toContainText("Tools found");
    await expect(issueTrackerRow).not.toContainText(
      "Automatic test interrupted",
    );
    await expect
      .poll(() =>
        page.evaluate(
          () =>
            JSON.parse(
              window.localStorage.getItem(
                "buzz.projects.pending-connection-automatic-tests.v1",
              ) ?? "[]",
            ).length,
        ),
      )
      .toBe(0);
  });

  test("scope switch ignores a pending successful delete", async ({ page }) => {
    await openConnections(page);
    const panel = page.getByTestId("project-connections-panel");
    await panel
      .getByRole("button", { name: "Remove Google Analytics" })
      .click();
    await page.evaluate(() => {
      window.__BUZZ_E2E_DEFER_NEXT_PROJECT_CONNECTION_DELETE__?.();
    });
    await page
      .getByRole("alertdialog", { name: "Remove Google Analytics?" })
      .getByRole("button", { name: "Remove connection" })
      .click();
    await expect
      .poll(() =>
        page.evaluate(
          () => window.__BUZZ_E2E_PROJECT_CONNECTION_DELETE_PENDING__ ?? 0,
        ),
      )
      .toBe(1);

    const emptyPanel = await switchToEmptyProject(page, true);
    expect(
      await page.evaluate(
        () => window.__BUZZ_E2E_RELEASE_PROJECT_CONNECTION_DELETE__?.() ?? 0,
      ),
    ).toBe(1);
    await expect
      .poll(() =>
        page.evaluate(
          () => window.__BUZZ_E2E_PROJECT_CONNECTION_DELETE_UI_SETTLED__ ?? 0,
        ),
      )
      .toBe(1);
    await expect(page.getByText("Google Analytics removed.")).toHaveCount(0);
    await expect(emptyPanel).not.toContainText("Google Analytics");
  });

  test("scope switch ignores a pending delete failure", async ({ page }) => {
    await openConnections(page);
    const panel = page.getByTestId("project-connections-panel");
    await panel
      .getByRole("button", { name: "Remove Google Analytics" })
      .click();
    await page.evaluate(() => {
      window.__BUZZ_E2E_DEFER_NEXT_PROJECT_CONNECTION_DELETE__?.();
    });
    await page
      .getByRole("alertdialog", { name: "Remove Google Analytics?" })
      .getByRole("button", { name: "Remove connection" })
      .click();
    await expect
      .poll(() =>
        page.evaluate(
          () => window.__BUZZ_E2E_PROJECT_CONNECTION_DELETE_PENDING__ ?? 0,
        ),
      )
      .toBe(1);

    const emptyPanel = await switchToEmptyProject(page, true);
    expect(
      await page.evaluate(
        () => window.__BUZZ_E2E_RELEASE_PROJECT_CONNECTION_DELETE__?.() ?? 0,
      ),
    ).toBe(1);
    await expect
      .poll(() =>
        page.evaluate(
          () => window.__BUZZ_E2E_PROJECT_CONNECTION_DELETE_UI_SETTLED__ ?? 0,
        ),
      )
      .toBe(1);
    await expect(
      page.getByText(/Couldn't remove Google Analytics/),
    ).toHaveCount(0);
    await expect(emptyPanel).not.toContainText("Google Analytics");
  });

  test("a Project with zero repositories can create and test a connection", async ({
    page,
  }) => {
    await openProject(page, "empty");

    const panel = page.getByTestId("project-connections-panel");
    await expect(panel).toBeVisible();
    await expect(panel).toContainText("No connections yet");
    await panel.getByRole("button", { name: "Add connection" }).first().click();
    const dialog = page.getByRole("dialog", {
      name: "Add Project connection to Empty Project",
    });
    await dialog.getByLabel("Connection name").fill("Linear");
    await dialog.getByLabel("Service").fill("Linear");
    await dialog
      .getByLabel("Connection command")
      .fill("/opt/homebrew/bin/linear-connector");
    await dialog
      .getByLabel(/I trust this executable and the arguments above/)
      .check();
    await dialog.getByRole("button", { name: "Save and test" }).click();

    await expect(panel).toContainText("Linear");
    await expect(panel).toContainText("Tools found");
  });
});
