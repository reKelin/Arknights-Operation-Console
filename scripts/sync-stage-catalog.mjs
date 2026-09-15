import { mkdir, writeFile } from "node:fs/promises";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const SOURCE_REPOSITORY = "Kengxxiao/ArknightsGameData";
const SOURCE_REVISION = "0ef7f952dfd018392200157a5c79a6511ba69122";
const BASE_URL = `https://cdn.jsdelivr.net/gh/${SOURCE_REPOSITORY}@${SOURCE_REVISION}/zh_CN/gamedata/excel`;
const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const OUTPUT = resolve(ROOT, "src-tauri/data/stages.json");

async function load(name) {
  let lastError;
  for (let attempt = 1; attempt <= 3; attempt += 1) {
    try {
      const response = await fetch(`${BASE_URL}/${name}`);
      if (!response.ok) {
        throw new Error(`HTTP ${response.status}`);
      }
      return await response.json();
    } catch (error) {
      lastError = error;
      if (attempt < 3) {
        await new Promise((resolveDelay) =>
          setTimeout(resolveDelay, attempt * 1000),
        );
      }
    }
  }
  throw new Error(`下载 ${name} 失败：${lastError}`);
}

function levelPath(levelId) {
  let path = levelId.replaceAll("\\", "/").toLowerCase();
  path = path.replace("/main/level_easy_sub", "/main/level_sub");
  path = path.replace("/main/level_easy", "/main/level_main");
  return `${path.replace(/^\/+/, "").replace(/\.json$/i, "")}.json`;
}

function add(entries, id, value) {
  if (
    typeof id !== "string" ||
    typeof value?.name !== "string" ||
    typeof value?.code !== "string" ||
    typeof value?.levelId !== "string"
  ) {
    return;
  }
  const name = value.name.trim();
  const code = value.code.trim();
  const levelId = value.levelId.trim();
  if (!id || !name || !levelId) {
    return;
  }
  entries.set(id, {
    id,
    code,
    name,
    levelPath: levelPath(levelId),
  });
}

const stageTable = await load("stage_table.json");
const roguelikeTable = await load("roguelike_topic_table.json");
const entries = new Map();
for (const [id, stage] of Object.entries(stageTable.stages ?? {})) {
  add(entries, id, stage);
}
for (const detail of Object.values(roguelikeTable.details ?? {})) {
  for (const [id, stage] of Object.entries(detail.stages ?? {})) {
    add(entries, id, stage);
  }
}

const stages = [...entries.values()].sort((left, right) =>
  left.id < right.id ? -1 : left.id > right.id ? 1 : 0,
);
const output = {
  schemaVersion: 1,
  sourceRepository: SOURCE_REPOSITORY,
  sourceRevision: SOURCE_REVISION,
  stages,
};
await mkdir(dirname(OUTPUT), { recursive: true });
await writeFile(OUTPUT, `${JSON.stringify(output, null, 2)}\n`, "utf8");
console.log(`写入 ${stages.length} 条关卡：${OUTPUT}`);
