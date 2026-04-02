/**
 * MCP Extension — connects to the Elle MCP server over stdio JSON-RPC 2.0
 *
 * Spawns `elle tools/mcp-server.lisp`, discovers tools via `tools/list`,
 * and registers each as a pi tool. The LLM can then call them directly.
 */

import type { ExtensionAPI } from "@mariozechner/pi-coding-agent";
import { Type, type TSchema } from "@sinclair/typebox";
import { spawn, type ChildProcess } from "node:child_process";
import { resolve } from "node:path";

interface McpTool {
  name: string;
  description: string;
  inputSchema?: Record<string, any>;
}

interface PendingRequest {
  resolve: (value: any) => void;
  reject: (reason: any) => void;
}

export default function mcpExtension(pi: ExtensionAPI) {
  let server: ChildProcess | null = null;
  let nextId = 1;
  const pending = new Map<number, PendingRequest>();
  let lineBuf = "";

  function handleLine(line: string) {
    if (!line.trim()) return;
    try {
      const msg = JSON.parse(line);
      // Notifications (no id) — log and ignore
      if (msg.id === undefined || msg.id === null) return;
      const p = pending.get(msg.id);
      if (!p) return;
      pending.delete(msg.id);
      if (msg.error) {
        p.reject(new Error(msg.error.message ?? JSON.stringify(msg.error)));
      } else {
        p.resolve(msg.result);
      }
    } catch {
      // Not JSON or not for us — ignore
    }
  }

  function rpc(method: string, params?: any): Promise<any> {
    return new Promise((resolve, reject) => {
      if (!server?.stdin?.writable) {
        return reject(new Error("MCP server not running"));
      }
      const id = nextId++;
      pending.set(id, { resolve, reject });
      const msg = JSON.stringify({ jsonrpc: "2.0", id, method, params: params ?? {} }) + "\n";
      server.stdin.write(msg);
    });
  }

  /** Convert JSON Schema object to TypeBox TSchema */
  function jsonSchemaToTypebox(schema: Record<string, any>): TSchema {
    if (!schema || !schema.properties) return Type.Object({});
    const props: Record<string, TSchema> = {};
    const required = new Set<string>(schema.required ?? []);
    for (const [key, prop] of Object.entries(schema.properties)) {
      const p = prop as Record<string, any>;
      let field: TSchema;
      if (p.enum) {
        field = Type.Union(p.enum.map((v: string) => Type.Literal(v)), { description: p.description });
      } else if (p.type === "string") {
        field = Type.String({ description: p.description });
      } else if (p.type === "integer") {
        field = Type.Integer({ description: p.description });
      } else if (p.type === "number") {
        field = Type.Number({ description: p.description });
      } else if (p.type === "boolean") {
        field = Type.Boolean({ description: p.description });
      } else if (p.type === "array") {
        const items = p.items ? jsonSchemaToTypebox(p.items) : Type.Any();
        field = Type.Array(items, { description: p.description });
      } else if (p.type === "object") {
        field = jsonSchemaToTypebox(p);
      } else {
        field = Type.Any({ description: p.description });
      }
      props[key] = required.has(key) ? field : Type.Optional(field);
    }
    return Type.Object(props);
  }

  function startServer(cwd: string): ChildProcess {
    const script = resolve(cwd, "tools/mcp-server.lisp");
    const proc = spawn("elle", [script], {
      cwd,
      stdio: ["pipe", "pipe", "pipe"],
      env: { ...process.env },
    });

    proc.stdout!.on("data", (chunk: Buffer) => {
      lineBuf += chunk.toString();
      const lines = lineBuf.split("\n");
      lineBuf = lines.pop()!; // Keep incomplete trailing line
      for (const line of lines) {
        handleLine(line);
      }
    });

    proc.stderr!.on("data", (chunk: Buffer) => {
      // Log server stderr for diagnostics
      for (const line of chunk.toString().split("\n")) {
        if (line.trim()) {
          console.error(`[mcp] ${line}`);
        }
      }
    });

    proc.on("exit", (code) => {
      console.error(`[mcp] server exited with code ${code}`);
      // Reject all pending requests
      for (const [id, p] of pending) {
        p.reject(new Error(`MCP server exited (code ${code})`));
        pending.delete(id);
      }
      server = null;
    });

    return proc;
  }

  pi.on("session_start", async (_event, ctx) => {
    try {
      server = startServer(ctx.cwd);

      // Initialize the MCP session
      await rpc("initialize", {
        protocolVersion: "2025-03-26",
        capabilities: {},
        clientInfo: { name: "pi-mcp-extension", version: "1.0.0" },
      });

      // Discover tools
      const { tools } = await rpc("tools/list", {});

      // Register each MCP tool as a pi tool
      for (const tool of tools as McpTool[]) {
        const schema = tool.inputSchema
          ? jsonSchemaToTypebox(tool.inputSchema)
          : Type.Object({});

        pi.registerTool({
          name: `mcp_${tool.name}`,
          label: `MCP: ${tool.name}`,
          description: tool.description,
          promptSnippet: tool.description,
          parameters: schema,
          async execute(_toolCallId, params) {
            const result = await rpc("tools/call", {
              name: tool.name,
              arguments: params,
            });
            const content = result?.content ?? [];
            const text = content
              .map((c: any) => c.text ?? JSON.stringify(c))
              .join("\n");
            const isError = result?.isError === true ||
              content.some((c: any) => c.isError === true);
            if (isError) {
              throw new Error(text);
            }
            return {
              content: [{ type: "text", text }],
              details: result,
            };
          },
        });
      }

      ctx.ui.notify(`MCP: ${(tools as McpTool[]).length} tools registered`, "info");
    } catch (err: any) {
      ctx.ui.notify(`MCP startup failed: ${err.message}`, "error");
      server?.kill();
      server = null;
    }
  });

  // Provide a /mcp-status command
  pi.registerCommand("mcp-status", {
    description: "Show MCP server status",
    handler: async (_args, ctx) => {
      if (server && !server.killed) {
        try {
          await rpc("ping", {});
          ctx.ui.notify("MCP server: running (ping ok)", "success");
        } catch (err: any) {
          ctx.ui.notify(`MCP server: error — ${err.message}`, "error");
        }
      } else {
        ctx.ui.notify("MCP server: not running", "warning");
      }
    },
  });

  // Provide a /mcp-restart command
  pi.registerCommand("mcp-restart", {
    description: "Restart the MCP server and re-register tools",
    handler: async (_args, ctx) => {
      server?.kill();
      server = null;
      ctx.ui.notify("MCP server stopped. Use /reload to reconnect.", "info");
    },
  });

  pi.on("session_shutdown", async () => {
    server?.kill();
    server = null;
  });
}
