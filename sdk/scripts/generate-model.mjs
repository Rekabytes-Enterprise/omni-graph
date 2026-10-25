import { readFileSync, writeFileSync } from "node:fs";
const source = readFileSync(
  new URL("../../src/model.rs", import.meta.url),
  "utf8",
);
const names = [
  "Scope",
  "Kind",
  "Status",
  "Evidence",
  "Visibility",
  "Progress",
  "Relation",
];
const snake = (value) =>
  value.replace(/[A-Z]/g, (m, i) => (i ? "_" : "") + m.toLowerCase());
let output =
  "// Generated from src/model.rs. Run npm run generate in sdk after model changes.\n";
for (const name of names) {
  const block = source.match(
    new RegExp(`enums!\\(${name} \\{([\\s\\S]*?)\\}\\);`),
  );
  if (!block) throw new Error(`Missing enum ${name}`);
  const values = block[1]
    .split(",")
    .map((x) => x.trim())
    .filter(Boolean)
    .map(snake);
  output += `export type ${name} = ${values.map(JSON.stringify).join(" | ")};\n`;
}
const fields = source.match(/pub struct MemoryInput \{([\s\S]*?)\n\}/)?.[1];
if (!fields) throw new Error("Missing MemoryInput");
output += "export interface MemoryFields {\n";
for (const match of fields.matchAll(/pub (\w+): ([^,\n]+),/g)) {
  const [, name, rust] = match;
  const field = name === "kind" ? "type" : name;
  let type =
    rust === "String"
      ? "string"
      : rust === "Vec<String>"
        ? "string[]"
        : rust === "Option<String>"
          ? "string | null"
          : rust === "Option<DateTime<Utc>>"
            ? "string | null"
            : rust === "Option<Progress>"
              ? "Progress | null"
              : rust;
  if (
    ![
      "string",
      "string[]",
      "string | null",
      "Progress | null",
      ...names,
    ].includes(type)
  )
    throw new Error(`Unsupported type ${rust}`);
  output += `  ${field}: ${type};\n`;
}
output += "}\n";
const target = new URL("../src/model.generated.ts", import.meta.url);
if (process.argv.includes("--check")) {
  if (readFileSync(target, "utf8") !== output)
    throw new Error("SDK model types are stale; run npm run generate");
} else writeFileSync(target, output);
