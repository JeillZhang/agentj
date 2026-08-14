import { compile } from "json-schema-to-typescript";
import { readdir, readFile, writeFile } from "node:fs/promises";
import { join } from "node:path";

const schemasDir = join(process.cwd(), "..", "..", "schemas");
const files = (await readdir(schemasDir)).filter((f) => f.endsWith(".json"));
const decls = [];
for (const f of files) {
  const schema = JSON.parse(await readFile(join(schemasDir, f), "utf8"));
  const name = f.replace(/\.json$/, "");
  decls.push(await compile(schema, name, { bannerComment: "" }));
}

// Several schemas both exist as standalone files AND are inlined as local
// `$defs` inside referencing schemas (e.g. ModuleState.json and ModuleInfo.json
// both define `ModuleState`). json-schema-to-typescript hoists every `$defs`
// entry to a top-level export, so the concatenated output can contain duplicate
// declarations. Keep the first occurrence of each exported name so the
// generated `types.d.ts` compiles cleanly under `tsc --noEmit`.
function dedupe(text) {
  const seen = new Set();
  const out = [];
  const lines = text.split("\n");
  let i = 0;
  while (i < lines.length) {
    const header = lines[i].match(/^export (interface|type) (\w+)/);
    if (!header) {
      out.push(lines[i]);
      i += 1;
      continue;
    }
    const [, kind, name] = header;
    let end = i + 1;
    if (kind === "type") {
      let depth = 0;
      for (let j = i; j < lines.length; j++) {
        depth += (lines[j].match(/{/g) || []).length;
        depth -= (lines[j].match(/}/g) || []).length;
        if (depth === 0 && lines[j].endsWith(";")) {
          end = j + 1;
          break;
        }
      }
    } else {
      let depth = 1;
      while (end < lines.length) {
        depth += (lines[end].match(/{/g) || []).length;
        depth -= (lines[end].match(/}/g) || []).length;
        end += 1;
        if (depth === 0) break;
      }
    }
    if (!seen.has(name)) {
      seen.add(name);
      out.push(...lines.slice(i, end));
    }
    i = end;
  }
  return out.join("\n");
}

await writeFile(join(process.cwd(), "types.d.ts"), dedupe(decls.join("\n")));
