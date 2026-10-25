// Generated from src/model.rs. Run npm run generate in sdk after model changes.
export type Scope = "global" | "client" | "project" | "task";
export type Kind = "preference" | "fact" | "decision" | "constraint" | "procedure" | "lesson" | "issue" | "handoff" | "mistake" | "fix" | "learning_goal" | "component" | "technology";
export type Status = "active" | "draft" | "superseded" | "archived";
export type Evidence = "verified" | "reported" | "inferred";
export type Visibility = "private" | "project_shared";
export type Progress = "open" | "investigating" | "fix_proposed" | "fixed" | "verified";
export type Relation = "belongs_to" | "applies_to" | "depends_on" | "supersedes" | "derived_from" | "contradicts" | "related_to" | "corrected_by" | "teaches" | "resolved_by";
export interface MemoryFields {
  title: string;
  body: string;
  summary: string;
  type: Kind;
  scope: Scope;
  task_id: string | null;
  topics: string[];
  keywords: string[];
  triggers: string[];
  status: Status;
  evidence_level: Evidence;
  sources: string[];
  created_by: string;
  visibility: Visibility;
  review_after: string | null;
  expires_at: string | null;
  context: string | null;
  failure: string | null;
  cause: string | null;
  prevention: string[];
  verification: string[];
  progress: Progress | null;
}
