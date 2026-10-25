// Compile-only examples protect the public declaration surface. This module is not exported.
import type { OmniGraph, MemoryInput, CreateLink } from "./index.js";
async function consume(client: OmniGraph): Promise<string[]> {
  const items = await client.list({ scope: "project" });
  return items.flatMap((memory) => [
    ...memory.prevention,
    ...memory.verification,
    memory.project_id ?? "",
    memory.task_id ?? "",
  ]);
}
const input: MemoryInput = {
  title: "Typed lesson",
  type: "lesson",
  scope: "project",
  progress: null,
};
const link: CreateLink = { relation: "related_to", to: "memory-id" };
// @ts-expect-error Both endpoints cannot be supplied in an atomic creation link.
const invalidLink: CreateLink = {
  relation: "related_to",
  from: "one",
  to: "two",
};
void consume;
void input;
void link;
void invalidLink;
