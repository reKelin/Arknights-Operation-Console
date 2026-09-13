import { spawnSync } from "node:child_process";
import { readFileSync, writeFileSync } from "node:fs";

if (!process.argv.includes("--normalize-only")) {
  const result = spawnSync(
    "cargo",
    [
      "run",
      "--no-default-features",
      "--manifest-path",
      "src-tauri/Cargo.toml",
      "--bin",
      "export-bindings",
    ],
    {
      env: { ...process.env, EXPORT_BINDINGS: "1" },
      stdio: "inherit",
      shell: process.platform === "win32",
    },
  );

  if (result.error) {
    throw result.error;
  }
  if (result.status !== 0) {
    process.exit(result.status ?? 1);
  }
}

const output = new URL("../src/generated/bindings.ts", import.meta.url);
const contents = readFileSync(output, "utf8");
writeFileSync(output, `${contents.trimEnd()}\n`);
