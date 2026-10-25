import { isAbsolute } from "node:path";
import type {
  MemoryFields,
  Scope,
  Kind,
  Status,
  Relation,
} from "./model.generated.js";
import { invoke } from "./transport.js";
import type { TransportOptions } from "./transport.js";
import {
  OmniGraphError,
  array,
  object,
  boolean,
  integer,
  parseCompatibility,
  parseMemory,
  parseHit,
  parseRecall,
  parseShow,
  parseEdge,
  protocol,
} from "./protocol.js";
import type { CompatibilityReport } from "./protocol.js";
export { OmniGraphError } from "./protocol.js";
export type {
  CompatibilityReport,
  StoreReport,
  StoreStatus,
  Memory,
  Edge,
} from "./protocol.js";
export type * from "./model.generated.js";
export interface Options {
  executable: string;
  directory: string;
  env?: Record<string, string | undefined>;
  migration?: "supported" | "manual";
  initialize?: boolean;
  timeoutMs?: number;
  maxOutputBytes?: number;
}
export interface Filter {
  scope?: Scope;
  type?: Kind;
  status?: Status;
  task?: string;
  topic?: string;
  limit?: number;
  all?: boolean;
}
export type CreateLink =
  | { relation: Relation; from: string; to?: never }
  | { relation: Relation; to: string; from?: never };
export type MemoryInput = Pick<MemoryFields, "title"> &
  Partial<Omit<MemoryFields, "title">> & {
    links?: CreateLink[];
    images?: { file: string; caption?: string }[];
  };
export type MemoryPatch = Partial<
  Omit<MemoryFields, "scope" | "task_id" | "created_by">
>;
function filterArgs(filter: Filter): string[] {
  const args: string[] = [];
  for (const [key, value] of Object.entries(filter)) {
    if (value === undefined) continue;
    if (key === "all") {
      if (value) args.push("--all");
    } else args.push(`--${key}`, String(value));
  }
  return args;
}
function required(report: CompatibilityReport, capability: string): void {
  if (!report.capabilities.includes(capability))
    throw new OmniGraphError(
      "unsupported_capability",
      `The executable does not support ${capability}.`,
      "Update the configured Omni-graph executable.",
    );
}
export class OmniGraph {
  private readonly transport: TransportOptions;
  private readonly migration: "supported" | "manual";
  private readonly initialize: boolean;
  private pending: Promise<CompatibilityReport> | undefined;
  constructor(options: Options) {
    if (!isAbsolute(options.executable) || !isAbsolute(options.directory))
      throw new OmniGraphError(
        "invalid_options",
        "executable and directory must be absolute paths.",
      );
    const timeoutMs = options.timeoutMs ?? 30_000,
      maxOutputBytes = options.maxOutputBytes ?? 16 * 1024 * 1024;
    if (
      !Number.isSafeInteger(timeoutMs) ||
      timeoutMs <= 0 ||
      !Number.isSafeInteger(maxOutputBytes) ||
      maxOutputBytes <= 0
    )
      throw new OmniGraphError(
        "invalid_options",
        "Timeout and output limits must be positive integers.",
      );
    this.transport = {
      executable: options.executable,
      directory: options.directory,
      env: { ...process.env, ...options.env },
      timeoutMs,
      maxOutputBytes,
    };
    this.migration = options.migration ?? "supported";
    this.initialize = options.initialize ?? false;
  }
  /** Read-only inspection. Never initializes or migrates a store. */
  async inspect(): Promise<CompatibilityReport> {
    const out = await invoke(this.transport, ["compatibility"]);
    return parseCompatibility(out.value, out.exit);
  }
  /** Recheck the configured executable and prepare stores according to the connection policy. */
  connect(): Promise<CompatibilityReport> {
    if (this.pending) return this.pending;
    const pending = this.prepareConnection();
    this.pending = pending;
    void pending.then(
      () => {
        if (this.pending === pending) this.pending = undefined;
      },
      () => {
        if (this.pending === pending) this.pending = undefined;
      },
    );
    return pending;
  }
  reconnect(): Promise<CompatibilityReport> {
    return this.connect();
  }
  private async prepareConnection(): Promise<CompatibilityReport> {
    let report = await this.inspect();
    if (report.status === "compatible") return report;
    const eligible = report.stores.every(
      (store) =>
        store.status === "compatible" ||
        (store.status === "migration_required" &&
          store.migration_supported &&
          this.migration === "supported") ||
        ((store.status === "missing_store" ||
          store.status === "legacy_migration_required") &&
          this.initialize),
    );
    if (!eligible)
      throw new OmniGraphError(
        "connection_not_ready",
        "Memory storage is not ready.",
        "Inspect details for each store. Supported migrations require migration: supported; initialization and legacy import require initialize: true.",
        report.exit_code,
        report,
      );
    required(report, "storage_preparation");
    const out = await invoke(this.transport, [
      "prepare",
      ...(this.initialize ? ["--initialize"] : []),
    ]);
    const preparation = object(out.value);
    if (
      preparation.integration_api_version !== 1 ||
      integer(preparation.exit_code) !== out.exit
    )
      throw protocol();
    const ready = boolean(preparation.ready);
    // Reinspect the actual selected stores instead of trusting a stale preparation result.
    report = await this.inspect();
    if (out.exit !== 0 || !ready || report.status !== "compatible")
      throw new OmniGraphError(
        "preparation_failed",
        "Memory storage preparation did not complete.",
        "Some stores may already be migrated. Inspect details, resolve the reported problem, and reconnect; do not recreate stores.",
        out.exit,
        preparation,
      );
    return report;
  }
  private async operation(
    args: string[],
    capability: string,
    input?: unknown,
  ): Promise<unknown> {
    const report = await this.connect();
    required(report, capability);
    const result = await invoke(this.transport, args, input);
    if (result.exit !== 0)
      throw new OmniGraphError(
        "operation_failed",
        "Omni-graph returned an unsuccessful response.",
        "Inspect saved state before retrying a write.",
        result.exit,
      );
    return result.value;
  }
  async list(filter: Filter = {}) {
    return array(
      await this.operation(["list", ...filterArgs(filter)], "memory_crud"),
      parseMemory,
    );
  }
  async search(query: string, filter: Filter = {}) {
    return array(
      await this.operation(
        ["search", ...filterArgs(filter), "--", query],
        "fts5_search",
      ),
      parseHit,
    );
  }
  async recall(query: string, options: Filter & { depth?: number } = {}) {
    const { depth = 1, ...filter } = options;
    return parseRecall(
      await this.operation(
        [
          "recall",
          ...filterArgs(filter),
          "--depth",
          String(depth),
          "--",
          query,
        ],
        "graph_recall",
      ),
    );
  }
  async show(id: string) {
    return parseShow(await this.operation(["show", "--", id], "memory_crud"));
  }
  async remember(
    input: MemoryInput,
    options: { idempotencyKey?: string } = {},
  ) {
    const key = options.idempotencyKey;
    if (key !== undefined && !/^[A-Za-z0-9._:-]{1,128}$/.test(key))
      throw new OmniGraphError("invalid_input", "Invalid idempotency key.");
    return parseMemory(
      await this.operation(
        [
          "remember",
          "--file",
          "-",
          ...(key === undefined ? [] : ["--idempotency-key", key]),
        ],
        key === undefined ? "memory_crud" : "idempotent_creation",
        input,
      ),
    );
  }
  async update(id: string, patch: MemoryPatch) {
    return parseMemory(
      await this.operation(
        ["update", "--file", "-", "--", id],
        "memory_crud",
        patch,
      ),
    );
  }
  async link(edge: { from: string; relation: Relation; to: string }) {
    return parseEdge(
      await this.operation(
        ["link", "--", edge.from, edge.relation, edge.to],
        "atomic_memory_links",
      ),
    );
  }
}
