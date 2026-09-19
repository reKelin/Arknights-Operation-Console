import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { readFileSync } from "node:fs";
import test from "node:test";

const read = (path) => readFileSync(path, "utf8");
const manifest = JSON.parse(read("package.json"));
const lock = JSON.parse(read("package-lock.json"));

test("新增测试依赖与清单一致，锁定版本和完整性", () => {
  assert.deepEqual(lock.packages[""].devDependencies, manifest.devDependencies);
  assert.deepEqual(lock.packages[""].dependencies, manifest.dependencies);
  for (const name of ["@playwright/test", "playwright", "playwright-core"]) {
    const entry = lock.packages[`node_modules/${name}`];
    assert.equal(entry.version, manifest.devDependencies["@playwright/test"]);
    assert.match(entry.integrity, /^sha512-/);
  }
});

test("Vitest 与浏览器冒烟使用独立入口", () => {
  assert.equal(manifest.scripts.test, "vitest run src");
  assert.equal(manifest.scripts["test:smoke"], "playwright test");
  assert.equal(
    manifest.scripts["app:build:local"],
    "tauri build --debug --no-bundle",
  );
});

test("PR 流程不执行原生构建、远程目录下载或发布", () => {
  const smoke = read(".github/workflows/ci.yml");
  assert.match(smoke, /pull_request:/);
  assert.match(smoke, /name: Smoke/);
  assert.match(smoke, /npm run test:smoke/);
  assert.doesNotMatch(
    smoke,
    /cargo (?:test|clippy|build)|npm run tauri|stages:sync|gh release/,
  );
  assert.doesNotMatch(smoke, /timeout-minutes: 5\b/);
});

test("重型流程只接受 tag 与手动触发，私有仓库有保护", () => {
  const release = read(".github/workflows/release.yml");
  const triggers = release.split("permissions:")[0];
  assert.match(triggers, /tags: \['v\*'\]/);
  assert.match(triggers, /workflow_dispatch:/);
  assert.doesNotMatch(triggers, /pull_request|release:|branches:|schedule:/);
  assert.match(release, /!github\.event\.repository\.private/);
  assert.match(release, /needs: \[smoke, validate-build\]/);
  assert.match(release, /sha256sum --check SHA256SUMS/);
  assert.ok(
    release.indexOf("gh release create") < release.indexOf("gh release edit"),
  );
  assert.match(release, /--draft/);
  assert.doesNotMatch(release, /timeout-minutes: 30\b/);
});

test("只允许耗时报告不阻塞，不忽略功能检查失败", () => {
  for (const file of ["ci.yml", "release.yml"]) {
    const text = read(`.github/workflows/${file}`);
    assert.equal((text.match(/continue-on-error: true/g) ?? []).length, 1);
    assert.match(
      text,
      /name: Report duration trend\n\s+if:.*\n\s+continue-on-error: true/,
    );
  }
});

test("发布版本必须与 tag 一致，不在校验失败时构建", () => {
  const version = manifest.version;
  const cases = [
    [`refs/tags/v${version}`, 0],
    ["refs/heads/main", 0],
    ["refs/tags/v0.0.0-invalid", 1],
  ];
  for (const [tag, expected] of cases) {
    const result = spawnSync(
      process.execPath,
      ["scripts/ci/check-release-version.mjs"],
      { env: { ...process.env, GITHUB_REF: tag }, encoding: "utf8" },
    );
    assert.equal(result.status, expected, result.stderr);
  }
});

test("数据下载不会把 GitHub 令牌发给外部 CDN", () => {
  assert.match(
    read("scripts/sync-stage-catalog.mjs"),
    /new URL\(url\)\.hostname === "api\.github\.com"/,
  );
});
