import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { mkdtempSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import {
  analyzeDurations,
  jobSeconds,
  readStepTimings,
  reportTiming,
} from "./timing.mjs";

test("单次慢或历史不足不提示优化", () => {
  assert.equal(analyzeDurations([500], 300).shouldOptimize, false);
  assert.equal(analyzeDurations([500, 400], 300).shouldOptimize, false);
});

test("只有最近三次均偏慢才提示，不使用某一次最坏耗时", () => {
  assert.equal(
    analyzeDurations([500, 450, 400, 100], 300).shouldOptimize,
    true,
  );
  assert.equal(
    analyzeDurations([200, 450, 400, 500], 300).shouldOptimize,
    false,
  );
  assert.equal(analyzeDurations([301, 300, 500], 300).shouldOptimize, false);
});

test("最多比较五个样本并计算中位数", () => {
  const result = analyzeDurations([100, 200, 300, 400, 500, 9999], 300);
  assert.deepEqual(result.samples, [100, 200, 300, 400, 500]);
  assert.equal(analyzeDurations([100, 200, 300, 400], 300).median, 250);
  assert.equal(analyzeDurations([100, 200, 300], 300).median, 200);
  assert.equal(analyzeDurations([], 300).median, null);
});

test("非法历史数据不当作快速或慢速成功", () => {
  const invalid = [null, undefined, Number.NaN, -1, Infinity];
  const result = analyzeDurations(invalid, 300);
  assert.deepEqual(result.samples, []);
});

test("任务耗时使用开始执行时间而非排队时间", () => {
  const duration = jobSeconds({
    created_at: "2026-01-01T00:00:00Z",
    started_at: "2026-01-01T00:05:00Z",
    completed_at: "2026-01-01T00:07:00Z",
  });
  assert.equal(duration, 120);
  assert.equal(jobSeconds({ started_at: "invalid", completed_at: null }), null);
  const negative = jobSeconds({
    started_at: "2026-01-01T00:05:00Z",
    completed_at: "2026-01-01T00:00:00Z",
  });
  assert.equal(negative, null);
});

test("计时包装保留失败退出码并记录成功与失败", () => {
  const directory = mkdtempSync(join(tmpdir(), "console-ci-"));
  const path = join(directory, "timings.jsonl");
  try {
    for (const code of [0, 7]) {
      const result = spawnSync(
        process.execPath,
        [
          "scripts/ci/run-timed.mjs",
          "test-step",
          "--",
          process.execPath,
          "-e",
          `process.exit(${code})`,
        ],
        { env: { ...process.env, CI_TIMINGS_FILE: path }, encoding: "utf8" },
      );
      assert.equal(result.status, code, result.stderr);
    }
    const records = readStepTimings(path);
    assert.deepEqual(
      records.map((record) => record.exitCode),
      [0, 7],
    );
    assert.equal(
      records.every((record) => record.seconds >= 0),
      true,
    );
    assert.equal(readFileSync(path, "utf8").includes("GITHUB_TOKEN"), false);
  } finally {
    rmSync(directory, { recursive: true });
  }
});

test("计时包装不接受缺失命令", () => {
  const result = spawnSync(process.execPath, ["scripts/ci/run-timed.mjs"], {
    encoding: "utf8",
  });
  assert.equal(result.status, 2);
});

function summaryCore() {
  const text = [];
  const warnings = [];
  const summary = {
    addHeading(value) {
      text.push(value);
      return this;
    },
    addTable() {
      return this;
    },
    addRaw(value) {
      text.push(value);
      return this;
    },
    async write() {
      text.push("written");
    },
  };
  return {
    core: { summary, warning: (value) => warnings.push(value) },
    text,
    warnings,
  };
}

test("Actions 历史读取失败不使测试失败", async () => {
  const { core, text, warnings } = summaryCore();
  const actions = {
    async getWorkflowRun() {
      throw new Error("forbidden");
    },
  };
  await reportTiming({
    github: { rest: { actions } },
    context: {
      repo: { owner: "owner", repo: "repo" },
      runId: 1,
      ref: "refs/pull/1/merge",
    },
    core,
    jobName: "Smoke",
    workflowFile: "ci.yml",
    referenceSeconds: 300,
    outcome: "success",
  });
  assert.equal(warnings.length, 0);
  assert.match(text.join(""), /未取得可比较/);
  assert.equal(text.at(-1), "written");
});

test("连续三次慢任务仅告警，过滤其他仓库和失败运行", async () => {
  const { core, warnings } = summaryCore();
  const requestedRuns = [];
  const actions = {
    async getWorkflowRun() {
      return {
        data: {
          event: "pull_request",
          head_branch: "feature",
          head_repository: { id: 1 },
        },
      };
    },
    async listWorkflowRuns(options) {
      assert.equal(options.branch, "feature");
      assert.equal(options.workflow_id, "ci.yml");
      return {
        data: {
          workflow_runs: [
            { id: 2, conclusion: "success", head_repository: { id: 1 } },
            { id: 3, conclusion: "success", head_repository: { id: 1 } },
            { id: 4, conclusion: "failure", head_repository: { id: 1 } },
            { id: 5, conclusion: "success", head_repository: { id: 99 } },
          ],
        },
      };
    },
    async listJobsForWorkflowRun({ run_id: id }) {
      requestedRuns.push(id);
      const job =
        id === 1
          ? {
              name: "Smoke",
              status: "in_progress",
              started_at: new Date(Date.now() - 400_000).toISOString(),
            }
          : {
              name: "Smoke",
              conclusion: "success",
              started_at: "2026-01-01T00:00:00Z",
              completed_at: "2026-01-01T00:07:00Z",
            };
      return { data: { jobs: [job] } };
    },
  };
  await reportTiming({
    github: { rest: { actions } },
    context: { repo: {}, runId: 1, ref: "refs/pull/1/merge" },
    core,
    jobName: "Smoke",
    workflowFile: "ci.yml",
    referenceSeconds: 300,
    outcome: "success",
  });
  assert.deepEqual(requestedRuns, [1, 2, 3]);
  assert.equal(warnings.length, 1);
});
