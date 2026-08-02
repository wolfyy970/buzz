import readline from "node:readline";

const requiredCanary = process.env.E007_ANALYTICS_CANARY;
if (!requiredCanary) {
  process.stderr.write("E007_ANALYTICS_CANARY is required\n");
  process.exit(2);
}

const summaries = {
  "portable-agents": {
    project: "portable-agents",
    period: "2026-W31",
    activeUsers: 128,
    completedRuns: 47,
    failedRuns: 3,
  },
  "product-alpha": {
    project: "product-alpha",
    period: "2026-W31",
    activeUsers: 128,
    completedRuns: 47,
    failedRuns: 3,
  },
  "product-beta": {
    project: "product-beta",
    period: "2026-W31",
    activeUsers: 91,
    completedRuns: 32,
    failedRuns: 1,
  },
};

const args = process.argv.slice(2);
let project = "portable-agents";
if (args.length !== 0) {
  if (args.length !== 2 || args[0] !== "--project") {
    process.stderr.write(
      "usage: synthetic-analytics-mcp.mjs [--project product-alpha|product-beta]\n",
    );
    process.exit(2);
  }
  project = args[1];
}
const weeklySummary = summaries[project];
if (!weeklySummary) {
  process.stderr.write("unknown synthetic analytics project\n");
  process.exit(2);
}

function respond(id, result) {
  process.stdout.write(`${JSON.stringify({ jsonrpc: "2.0", id, result })}\n`);
}

function respondError(id, code, message) {
  process.stdout.write(
    `${JSON.stringify({
      jsonrpc: "2.0",
      id,
      error: { code, message },
    })}\n`,
  );
}

function handleRequest(request) {
  if (request.method === "initialize") {
    respond(request.id, {
      protocolVersion: "2025-06-18",
      capabilities: { tools: {} },
      serverInfo: {
        name: "buzz-e007-synthetic-analytics",
        version: "1.0.0",
      },
    });
    return;
  }

  if (request.method === "notifications/initialized") {
    return;
  }

  if (request.method === "tools/list") {
    respond(request.id, {
      tools: [
        {
          name: "analytics.weekly_summary",
          description:
            "Return the fixed synthetic weekly analytics summary used by Buzz isolation tests.",
          inputSchema: {
            type: "object",
            properties: {},
            additionalProperties: false,
          },
        },
      ],
    });
    return;
  }

  if (request.method === "tools/call") {
    if (request.params?.name !== "analytics.weekly_summary") {
      respondError(request.id, -32602, "Unknown synthetic analytics tool.");
      return;
    }
    respond(request.id, {
      content: [
        {
          type: "text",
          text: JSON.stringify({
            ...weeklySummary,
            credentialAvailable: Boolean(requiredCanary),
          }),
        },
      ],
      isError: false,
    });
    return;
  }

  if (request.id != null) {
    respondError(request.id, -32601, "Method not found.");
  }
}

const input = readline.createInterface({
  input: process.stdin,
  crlfDelay: Number.POSITIVE_INFINITY,
});

input.on("line", (line) => {
  let request;
  try {
    request = JSON.parse(line);
  } catch {
    return;
  }
  handleRequest(request);
});
