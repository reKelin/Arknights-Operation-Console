import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";

const read = (path) => readFileSync(path, "utf8");

test("PR CI 保留 Rust 检查并锁定依赖", () => {
  const workflow = read(".github/workflows/ci.yml");
  assert.match(workflow, /merge_group:/);
  assert.match(workflow, /cancel-in-progress: true/);
  assert.match(workflow, /cargo fmt --check/);
  assert.match(workflow, /cargo clippy --locked --all-targets -- -D warnings/);
  assert.match(workflow, /cargo test --locked/);
});

test("目录同步不会把 GitHub 令牌发送给外部 CDN", () => {
  assert.match(
    read("scripts/sync-stage-catalog.mjs"),
    /new URL\(url\)\.hostname === "api\.github\.com"/,
  );
});
