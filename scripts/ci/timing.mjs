import { readFileSync } from "node:fs";

export function analyzeDurations(
  seconds,
  referenceSeconds,
  requiredSamples = 3,
) {
  const valid = seconds
    .filter((value) => Number.isFinite(value) && value >= 0)
    .slice(0, 5);
  const recent = valid.slice(0, requiredSamples);
  const sorted = [...valid].sort((a, b) => a - b);
  const middle = Math.floor(sorted.length / 2);
  let median = null;
  if (sorted.length > 0) {
    median =
      sorted.length % 2
        ? sorted[middle]
        : (sorted[middle - 1] + sorted[middle]) / 2;
  }
  return {
    samples: valid,
    median,
    shouldOptimize:
      recent.length === requiredSamples &&
      recent.every((value) => value > referenceSeconds),
  };
}

export function jobSeconds(job) {
  const start = Date.parse(job.started_at);
  const end = Date.parse(job.completed_at);
  if (!Number.isFinite(start) || !Number.isFinite(end) || end < start) {
    return null;
  }
  return (end - start) / 1000;
}

export function readStepTimings(path = ".local/ci-timings.jsonl") {
  try {
    return readFileSync(path, "utf8")
      .trim()
      .split("\n")
      .filter(Boolean)
      .map((line) => JSON.parse(line));
  } catch (error) {
    if (error.code === "ENOENT") return [];
    throw error;
  }
}

// 只读 Actions 元数据；耗时提示不能改变功能检查结果。
export async function reportTiming({
  github,
  context,
  core,
  jobName,
  workflowFile,
  referenceSeconds,
  outcome,
}) {
  core.summary.addHeading("耗时观察（非通过门槛）", 2);
  const timings = readStepTimings();
  if (timings.length) {
    core.summary.addTable([
      [
        { data: "步骤", header: true },
        { data: "秒", header: true },
        { data: "退出码", header: true },
      ],
      ...timings.map((step) => [
        step.name,
        step.seconds.toFixed(1),
        String(step.exitCode),
      ]),
    ]);
  }
  try {
    const args = { ...context.repo };
    const { data: current } = await github.rest.actions.getWorkflowRun({
      ...args,
      run_id: context.runId,
    });
    const { data: currentJobs } =
      await github.rest.actions.listJobsForWorkflowRun({
        ...args,
        run_id: context.runId,
        per_page: 100,
      });
    // 可复用 workflow 的检查名带有调用方前缀。
    const matchesName = (name) =>
      name === jobName || name.endsWith(` / ${jobName}`);
    const currentJob = currentJobs.jobs.find(
      (job) => matchesName(job.name) && job.status === "in_progress",
    );
    const currentSeconds = currentJob
      ? (Date.now() - Date.parse(currentJob.started_at)) / 1000
      : null;
    if (currentSeconds !== null && Number.isFinite(currentSeconds)) {
      core.summary.addRaw(
        `本次任务运行至报告步骤：${(currentSeconds / 60).toFixed(1)} 分钟，` +
          "包含依赖准备，不包含 runner 排队。\n\n",
      );
    }
    // tag 名称不同，比较同一 release workflow 的历史 tag；PR/main 比较同分支。
    const isTag =
      current.event === "push" && context.ref.startsWith("refs/tags/");
    const filters = isTag ? {} : { branch: current.head_branch };
    const { data } = await github.rest.actions.listWorkflowRuns({
      ...args,
      workflow_id: current.workflow_id ?? workflowFile,
      event: current.event,
      status: "completed",
      ...filters,
      per_page: 30,
    });
    const runs = data.workflow_runs
      .filter(
        (run) =>
          run.id !== context.runId &&
          run.conclusion === "success" &&
          run.head_repository?.id === current.head_repository?.id,
      )
      .slice(0, 5);
    const samples = [];
    for (const run of runs) {
      const { data: jobs } = await github.rest.actions.listJobsForWorkflowRun({
        ...args,
        run_id: run.id,
        per_page: 100,
      });
      const job = jobs.jobs.find(
        (item) => matchesName(item.name) && item.conclusion === "success",
      );
      const duration = job ? jobSeconds(job) : null;
      if (duration !== null) samples.push(duration);
    }
    if (
      outcome === "success" &&
      currentSeconds !== null &&
      Number.isFinite(currentSeconds)
    ) {
      samples.unshift(currentSeconds);
    }
    const result = analyzeDurations(samples, referenceSeconds);
    const sampleText = result.samples.length
      ? result.samples
          .map((value) => `${(value / 60).toFixed(1)} 分钟`)
          .join("、")
      : "暂无";
    core.summary.addRaw(
      `最近可比较的成功运行（新到旧，最多五次）：${sampleText}。\n\n`,
    );
    if (result.median !== null) {
      core.summary.addRaw(
        `中位数：${(result.median / 60).toFixed(1)} 分钟。\n\n`,
      );
    }
    if (result.shouldOptimize) {
      const message =
        `近期连续三次耗时均超过 ${referenceSeconds / 60} 分钟参考值，` +
        "建议检查冷缓存、依赖安装、编译和慢用例；不改变本次检查结论。";
      core.warning(message);
      core.summary.addRaw(`${message}\n\n`);
    } else {
      core.summary.addRaw(
        "单次偏慢或历史不足不告警；必要的功能检查不得为压缩耗时而删除。\n\n",
      );
    }
  } catch {
    // Fork 权限或 API 故障只影响趋势数据，不提升权限、不掩盖测试失败。
    core.summary.addRaw(
      "未取得可比较的 Actions 历史数据；保留本次步骤耗时，" +
        "不据此判断性能已达标。\n\n",
    );
  }
  await core.summary.write();
}
