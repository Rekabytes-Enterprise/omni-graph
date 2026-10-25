import { isAbsolute } from "node:path";
export class OmniGraphError extends Error {
  constructor(
    readonly code: string,
    message: string,
    readonly recovery = "",
    readonly exitCode: number | null = null,
    readonly details: unknown = null,
  ) {
    super(message);
    this.name = "OmniGraphError";
  }
}
export function object(value: unknown): Record<string, unknown> {
  if (typeof value !== "object" || value === null || Array.isArray(value))
    throw protocol();
  return Object.fromEntries(Object.entries(value));
}
export function protocol(): OmniGraphError {
  return new OmniGraphError(
    "invalid_response",
    "Omni-graph returned an invalid API response.",
    "Check the configured executable and its integration API support.",
  );
}
export function string(value: unknown): string {
  if (typeof value !== "string") throw protocol();
  return value;
}
export function number(value: unknown): number {
  if (typeof value !== "number" || !Number.isFinite(value)) throw protocol();
  return value;
}
export function integer(value: unknown): number {
  const n = number(value);
  if (!Number.isInteger(n)) throw protocol();
  return n;
}
export function boolean(value: unknown): boolean {
  if (typeof value !== "boolean") throw protocol();
  return value;
}
export function array<T>(value: unknown, parse: (value: unknown) => T): T[] {
  if (!Array.isArray(value)) throw protocol();
  return value.map(parse);
}
function nullableInteger(value: unknown): number | null {
  return value === null ? null : integer(value);
}
const statusCodes = {
  compatible: 0,
  missing_store: 10,
  migration_required: 11,
  legacy_migration_required: 11,
  unsupported_schema: 12,
  permission_denied: 13,
  invalid_database: 14,
  unrecognized_database: 14,
  busy: 15,
  io_error: 16,
};
export type StoreStatus = keyof typeof statusCodes;
function status(value: unknown): StoreStatus {
  switch (value) {
    case "compatible":
    case "missing_store":
    case "migration_required":
    case "legacy_migration_required":
    case "unsupported_schema":
    case "permission_denied":
    case "invalid_database":
    case "unrecognized_database":
    case "busy":
    case "io_error":
      return value;
    default:
      throw protocol();
  }
}
function absolute(value: unknown): string {
  const p = string(value);
  if (!isAbsolute(p)) throw protocol();
  return p;
}
export function parseStore(value: unknown) {
  const v = object(value);
  return {
    path: absolute(v.path),
    detected_schema_version: nullableInteger(v.detected_schema_version),
    status: status(v.status),
    migration_required: boolean(v.migration_required),
    migration_supported: boolean(v.migration_supported),
    recovery: string(v.recovery),
  };
}
export type StoreReport = ReturnType<typeof parseStore>;
export function parseCompatibility(value: unknown, exit: number) {
  const v = object(value),
    executable = object(v.executable),
    database = object(v.database);
  if (v.integration_api_version !== 1) throw protocol();
  const supported = array(v.supported_api_versions, integer);
  if (!supported.includes(1))
    throw new OmniGraphError(
      "unsupported_api_version",
      "The executable does not support integration API 1.",
    );
  const stores = array(v.stores, parseStore),
    overall = status(v.status),
    code = integer(v.exit_code);
  if (
    !stores.length ||
    code !== exit ||
    statusCodes[overall] !== code ||
    Math.max(...stores.map((s) => statusCodes[s.status])) !== code
  )
    throw protocol();
  return {
    integration_api_version: 1,
    executable: {
      version: string(executable.version),
      path: absolute(executable.path),
    },
    supported_api_versions: supported,
    capabilities: array(v.capabilities, string),
    database: {
      read_versions: array(database.read_versions, integer),
      write_versions: array(database.write_versions, integer),
      migratable_versions: array(database.migratable_versions, integer),
      migration_target: integer(database.migration_target),
      migration_policy: string(database.migration_policy),
    },
    stores,
    status: overall,
    exit_code: code,
  };
}
export type CompatibilityReport = ReturnType<typeof parseCompatibility>;
export function parseError(
  value: unknown,
  exit: number,
): OmniGraphError | null {
  const v = object(value);
  if (!("error" in v)) return null;
  const e = object(v.error);
  if (e.integration_api_version !== 1 || integer(e.exit_code) !== exit)
    throw protocol();
  return new OmniGraphError(
    string(e.code),
    string(e.message),
    string(e.recovery),
    exit,
  );
}
function nullableString(value: unknown): string | null {
  return value === null ? null : string(value);
}
export function parseMemory(value: unknown) {
  const v = object(value);
  // Enum values stay strings on output so additive engine variants remain readable.
  return {
    id: string(v.id),
    project_id: nullableString(v.project_id),
    client_id: nullableString(v.client_id),
    title: string(v.title),
    body: string(v.body),
    summary: string(v.summary),
    type: string(v.type),
    scope: string(v.scope),
    task_id: nullableString(v.task_id),
    status: string(v.status),
    evidence_level: string(v.evidence_level),
    visibility: string(v.visibility),
    created_by: string(v.created_by),
    sources: array(v.sources, string),
    topics: array(v.topics, string),
    keywords: array(v.keywords, string),
    triggers: array(v.triggers, string),
    prevention: array(v.prevention, string),
    verification: array(v.verification, string),
    created_at: string(v.created_at),
    updated_at: string(v.updated_at),
    verified_at: nullableString(v.verified_at),
    review_after: nullableString(v.review_after),
    expires_at: nullableString(v.expires_at),
    context: nullableString(v.context),
    failure: nullableString(v.failure),
    cause: nullableString(v.cause),
    progress: nullableString(v.progress),
  };
}
export type Memory = ReturnType<typeof parseMemory>;
export function parseEdge(value: unknown) {
  const v = object(value);
  return {
    from: string(v.from),
    relation: string(v.relation),
    to: string(v.to),
  };
}
export type Edge = ReturnType<typeof parseEdge>;
export function parseImage(value: unknown) {
  const v = object(value);
  return {
    id: string(v.id),
    memory_id: string(v.memory_id),
    name: string(v.name),
    caption: string(v.caption),
    media_type: string(v.media_type),
    bytes: integer(v.bytes),
    sha256: string(v.sha256),
  };
}
export function parseHit(value: unknown) {
  const v = object(value);
  return {
    memory: parseMemory(v.memory),
    score: number(v.score),
    reason: string(v.reason),
    images: array(v.images, parseImage),
  };
}
export function parseRecall(value: unknown) {
  const v = object(value);
  return {
    query: string(v.query),
    results: array(v.results, parseHit),
    edges: array(v.edges, parseEdge),
    guidance: string(v.guidance),
  };
}
export function parseShow(value: unknown) {
  const v = object(value);
  return {
    memory: parseMemory(v.memory),
    edges: array(v.edges, parseEdge),
    images: array(v.images, parseImage),
  };
}
