import { spawn } from "node:child_process";
import { OmniGraphError, parseError, protocol } from "./protocol.js";
export interface TransportOptions {
  executable: string;
  directory: string;
  env: NodeJS.ProcessEnv;
  timeoutMs: number;
  maxOutputBytes: number;
}
export function invoke(
  options: TransportOptions,
  args: string[],
  input?: unknown,
): Promise<{ value: unknown; exit: number }> {
  return new Promise((resolve, reject) => {
    let settled = false,
      length = 0;
    const chunks: Buffer[] = [];
    const child = spawn(
      options.executable,
      [
        "--directory",
        options.directory,
        "--api-version",
        "1",
        "--json",
        ...args,
      ],
      {
        cwd: options.directory,
        env: options.env,
        shell: false,
        stdio: ["pipe", "pipe", "pipe"],
        windowsHide: true,
      },
    );
    const finish = (error: Error) => {
      if (settled) return;
      settled = true;
      clearTimeout(timer);
      child.kill("SIGKILL");
      reject(error);
    };
    const timer = setTimeout(
      () =>
        finish(
          new OmniGraphError(
            "timeout",
            "Omni-graph did not finish before the timeout.",
            "A write may have committed. Inspect saved state or retry a keyed creation with the same key and payload.",
          ),
        ),
      options.timeoutMs,
    );
    child.on("error", () =>
      finish(
        new OmniGraphError(
          "executable_unavailable",
          "Cannot start the configured Omni-graph executable.",
          "Check its path and permissions.",
        ),
      ),
    );
    child.stdin.on("error", () => {}); // Exit handling below owns broken-pipe diagnostics.
    child.stdout.on("data", (chunk: Buffer) => {
      length += chunk.length;
      if (length > options.maxOutputBytes)
        finish(
          new OmniGraphError(
            "output_limit",
            "Omni-graph exceeded the response limit.",
            "Narrow the query or raise maxOutputBytes.",
          ),
        );
      else chunks.push(chunk);
    });
    child.stderr.on("data", (chunk: Buffer) => {
      length += chunk.length;
      if (length > options.maxOutputBytes)
        finish(
          new OmniGraphError(
            "output_limit",
            "Omni-graph exceeded the response limit.",
          ),
        );
    });
    child.on("close", (exit, signal) => {
      if (settled) return;
      settled = true;
      clearTimeout(timer);
      if (exit === null || signal) {
        reject(
          new OmniGraphError(
            "interrupted",
            "Omni-graph was interrupted.",
            "A write may have committed; inspect state before retrying.",
          ),
        );
        return;
      }
      try {
        const value: unknown = JSON.parse(
          Buffer.concat(chunks).toString("utf8"),
        );
        if (
          value !== null &&
          typeof value === "object" &&
          !Array.isArray(value)
        ) {
          const error = parseError(value, exit);
          if (error) throw error;
        }
        resolve({ value, exit });
      } catch (error) {
        reject(error instanceof OmniGraphError ? error : protocol());
      }
    });
    try {
      child.stdin.end(input === undefined ? undefined : JSON.stringify(input));
    } catch {
      finish(
        new OmniGraphError("invalid_input", "Input must be JSON serializable."),
      );
    }
  });
}
