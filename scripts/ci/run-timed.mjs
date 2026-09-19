import { spawn } from "node:child_process";
import { appendFileSync, mkdirSync } from "node:fs";
import { dirname } from "node:path";

const [name, separator, command, ...args] = process.argv.slice(2);
if (!name || separator !== "--" || !command) {
  console.error(
    "用法：node scripts/ci/run-timed.mjs <步骤名> -- <命令> [参数]",
  );
  process.exit(2);
}
const startedAt = new Date();
const start = performance.now();
const destination = process.env.CI_TIMINGS_FILE ?? ".local/ci-timings.jsonl";
let recorded = false;

function finish(code) {
  if (recorded) return;
  recorded = true;
  mkdirSync(dirname(destination), { recursive: true });
  const record = {
    name,
    startedAt: startedAt.toISOString(),
    seconds: Number(((performance.now() - start) / 1000).toFixed(3)),
    exitCode: code,
  };
  appendFileSync(destination, `${JSON.stringify(record)}\n`);
  process.exitCode = code;
}

const needsWindowsShell =
  process.platform === "win32" && !/\.(?:com|exe)$/i.test(command);
const child = spawn(command, args, {
  stdio: "inherit",
  // Windows npm.cmd 需要解释器；原生可执行文件直接启动，避免带空格路径被 shell 拆分。
  shell: needsWindowsShell,
});
child.on("error", (error) => {
  console.error(error.message);
  finish(1);
});
child.on("close", (code) => finish(code ?? 1));
