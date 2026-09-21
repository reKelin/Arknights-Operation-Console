import { expect, type Locator, type Page, test } from "@playwright/test";
import { installDesktopMock } from "../desktopMock";

const runtimeErrors = new WeakMap<Page, string[]>();

test.beforeEach(async ({ page }) => {
  const errors: string[] = [];
  runtimeErrors.set(page, errors);
  page.on("pageerror", (error) => errors.push(error.message));
  page.on("console", (message) => {
    if (message.type() === "error") errors.push(message.text());
  });
  await installDesktopMock(page);
  await page.goto("/");
  const liveMode = page.getByRole("tab", { name: "实时录轴", exact: true });
  await expect(liveMode).toHaveAttribute("aria-selected", "true");
});

test.afterEach(async ({ page }) => {
  expect(runtimeErrors.get(page)).toEqual([]);
  expect(await page.evaluate(() => window.__smoke.errors)).toEqual([]);
  await expect(page.locator(".error-toast")).toHaveCount(0);
});

async function selectAndBlur(select: Locator, value: string) {
  await select.selectOption(value);
  await select.focus();
  await select.press("Tab");
  await expect(select).toHaveValue(value);
}

async function openSettings(page: Page, tab: string) {
  await page.getByRole("button", { name: "打开设置", exact: true }).click();
  await page.getByRole("tab", { name: tab, exact: true }).click();
}

test("多选确认提交当前输入帧且清除全部已确认提示", async ({ page }) => {
  await page.evaluate(() => {
    const events = window.__smoke.snapshot.axis.events;
    for (const event of events) {
      event.timeConfirmation = "unconfirmed";
      event.frameRange = { start: event.frame - 10, end: event.frame + 10 };
    }
    const extra = events[1];
    if (extra)
      events.push({ ...structuredClone(extra), id: "unselected", frame: 150 });
  });
  await page.getByRole("tab", { name: "视频分析", exact: true }).click();
  await page.keyboard.press("h");
  const rows = page.locator(".editor-table tbody tr");
  await expect(rows).toHaveCount(3);
  await page.getByLabel("选择 技能 F90", { exact: true }).check();
  const input = page.locator('[name="edit-frame"]');
  await input.fill("95");
  const confirm = page.getByRole("button", { name: "确认时间", exact: true });
  await expect(confirm).toBeEnabled();
  await confirm.click();
  await expect(rows.nth(0)).not.toContainText("时间待确认");
  await expect(rows.nth(1)).not.toContainText("时间待确认");
  await expect(rows.nth(2)).toContainText("时间待确认");
  await expect(confirm).toHaveCount(0);
  expect(
    await page.evaluate(() =>
      window.__smoke.snapshot.axis.events.map((event) => event.frame),
    ),
  ).toEqual([30, 95, 150]);
  expect(
    await page.evaluate(() =>
      window.__smoke.calls
        .filter((call) => call.command === "confirm_event_times")
        .map((call) => call.args.inputs),
    ),
  ).toEqual([
    [
      { id: "event-deploy", frame: 30, manualCorrectionConfirmed: false },
      { id: "event-skill", frame: 95, manualCorrectionConfirmed: false },
    ],
  ]);
});

test("当前行已确认时仍能确认其他勾选项，范围外需人工校正", async ({ page }) => {
  await page.evaluate(() => {
    const first = window.__smoke.snapshot.axis.events[0];
    if (first) {
      first.timeConfirmation = "unconfirmed";
      first.frame = 35;
    }
  });
  await page.getByRole("tab", { name: "视频分析", exact: true }).click();
  await page.keyboard.press("h");
  await page.getByLabel("选择 技能 F90", { exact: true }).check();
  const confirm = page.getByRole("button", { name: "确认时间", exact: true });
  await expect(confirm).toBeDisabled();
  await page.getByLabel("确认所选操作中超出观测范围的人工校正").check();
  await expect(confirm).toBeEnabled();
  await confirm.click();
  await expect(
    page.locator(".editor-table tbody tr").first(),
  ).not.toContainText("时间待确认");
  await expect(confirm).toHaveCount(0);
});

test("已确认操作重新编辑时间可直接提交当前输入", async ({ page }) => {
  await page.evaluate(() => {
    const first = window.__smoke.snapshot.axis.events[0];
    if (first) first.frameRange = { start: 20, end: 50 };
  });
  await page.getByRole("tab", { name: "视频分析", exact: true }).click();
  await page.keyboard.press("h");
  const confirm = page.getByRole("button", { name: "确认时间", exact: true });
  await expect(confirm).toHaveCount(0);
  await page.locator('[name="edit-frame"]').fill("40");
  await expect(confirm).toBeEnabled();
  await confirm.click();
  await expect(page.locator(".editor-table tbody tr").first()).toContainText(
    "40f",
  );
  await expect(confirm).toHaveCount(0);
});

test("中文部署名称自动匹配，参数和时间提示分别清除", async ({ page }) => {
  await page.evaluate(() => {
    const first = window.__smoke.snapshot.axis.events[0];
    if (first) {
      first.timeConfirmation = "unconfirmed";
      first.operator = "望";
      first.complete = false;
    }
  });
  await page.getByRole("tab", { name: "视频分析", exact: true }).click();
  await page.keyboard.press("h");
  const row = page.locator(".editor-table tbody tr").first();
  const input = page.locator('[name="edit-operator"]');
  for (const [name, id] of [
    ["望", "char_2027_wang"],
    ["赤刃明霄陈", "char_1050_chen3"],
    ["棋子", "token_10064_wang_stone1"],
  ] as const) {
    await input.fill(name);
    await input.press("Tab");
    await expect
      .poll(() =>
        page.evaluate(() => window.__smoke.snapshot.axis.events[0]?.operator),
      )
      .toBe(id);
    await expect(input).toHaveValue(name);
    await expect(row).toContainText(name);
    await expect(row).not.toContainText("待补全参数");
    await expect(row).not.toContainText("未识别");
    await expect(row).toContainText("时间待确认");
  }
  await page.getByRole("button", { name: "确认时间", exact: true }).click();
  await expect(row).not.toContainText("时间待确认");
});

test("工作台加载及三种模式切换", async ({ page }) => {
  for (const name of ["代理指挥", "视频分析", "实时录轴"]) {
    const tab = page.getByRole("tab", { name, exact: true });
    await tab.click();
    await expect(tab).toHaveAttribute("aria-selected", "true");
  }
  const modes = await page.evaluate(() =>
    window.__smoke.calls
      .filter((call) => call.command === "set_console_mode")
      .map((call) => call.args.mode),
  );
  expect(modes).toEqual(["proxy", "recordingAnalysis", "manualRecording"]);
});

test("主题和轴版本下拉选择后实际更新页面", async ({ page }) => {
  await page.getByLabel("轴版本", { exact: true }).selectOption("revision-old");
  await expect(page.locator(".axis-title")).toHaveText("旧版本作战轴");
  await page
    .getByLabel("轴版本", { exact: true })
    .selectOption("revision-active");
  await expect(page.locator(".axis-title")).toHaveText("冒烟作战轴");
  await openSettings(page, "外观");
  await page.getByLabel("主题", { exact: true }).selectOption("light");
  await expect(page.locator("html")).toHaveAttribute("data-theme", "light");
  await page.getByRole("button", { name: "返回", exact: true }).click();
  await openSettings(page, "外观");
  await expect(page.getByLabel("主题", { exact: true })).toHaveValue("light");
});

test("整理页筛选、操作类型和朝向下拉会改变结果", async ({ page }) => {
  await page.keyboard.press("h");
  await expect(page.locator('[name="edit-direction"] option')).toHaveText([
    "待确认",
    "朝上",
    "朝右",
    "朝下",
    "朝左",
    "无朝向",
  ]);
  const rows = page.locator(".editor-table tbody tr");
  await page.getByLabel("筛选操作类型").selectOption("skill");
  await expect(rows).toHaveCount(1);
  await expect(rows.first()).toContainText("技能");
  await page.getByLabel("筛选操作类型").selectOption("all");
  await expect(rows).toHaveCount(2);
  await selectAndBlur(page.locator('[name="edit-direction"]'), "left");
  await expect(rows.first()).toContainText("朝左");
  await selectAndBlur(page.locator('[name="edit-type"]'), "skill");
  await expect(rows.first()).toContainText("技能");
  await expect(page.locator('[name="edit-direction"]')).toHaveCount(0);
  const kind = await page.evaluate(
    () => window.__smoke.snapshot.axis.events[0]?.kind,
  );
  expect(kind).toBe("skill");
});

test("轴信息关卡下拉选择进入保存命令", async ({ page }) => {
  await page.keyboard.press("h");
  await page.getByRole("button", { name: "轴信息", exact: true }).click();
  await page.getByRole("button", { name: "查找关卡", exact: true }).click();
  await page.getByLabel("关卡搜索结果").selectOption("smoke-stage-2");
  await page.getByRole("button", { name: "保存轴信息", exact: true }).click();
  await expect
    .poll(() => page.evaluate(() => window.__smoke.snapshot.axis.stageId))
    .toBe("smoke-stage-2");
});

test("新增操作、失焦保存后重新进入仍可读", async ({ page }) => {
  await page.keyboard.press("h");
  await page.getByRole("button", { name: "添加操作", exact: true }).click();
  await expect(page.locator(".editor-table tbody tr")).toHaveCount(3);
  const note = page.locator('[name="edit-note"]');
  await note.fill("冒烟编辑保存");
  await note.press("Tab");
  await expect
    .poll(() =>
      page.evaluate(() => window.__smoke.snapshot.axis.events.at(-1)?.label),
    )
    .toBe("冒烟编辑保存");
  await page.getByRole("button", { name: "返回", exact: true }).click();
  await page.keyboard.press("h");
  await expect(note).toHaveValue("冒烟编辑保存");
});

test("宽度与高度分别缩小和放大，布局跟随窗口", async ({ page }) => {
  let previousHeroHeight = 0;
  for (const size of [
    { width: 860, height: 280 },
    { width: 1280, height: 280 },
    { width: 1280, height: 700 },
    { width: 1280, height: 320 },
    { width: 860, height: 280 },
  ]) {
    await page.setViewportSize(size);
    await expect
      .poll(async () => {
        const bounds = await page.locator(".app-shell").boundingBox();
        return Math.round(bounds?.width ?? 0);
      })
      .toBe(size.width);
    const regions = [
      ".app-shell",
      ".titlebar",
      ".modebar",
      ".mode-hero",
      ".axis-heading",
      ".timeline-viewport",
    ];
    const boxes = await page
      .locator(regions.join(", "))
      .evaluateAll((elements) =>
        elements.map((element) => {
          const rect = element.getBoundingClientRect();
          return {
            left: rect.left,
            right: rect.right,
            bottom: rect.bottom,
            width: rect.width,
          };
        }),
      );
    for (const box of boxes) {
      expect(box.left).toBeGreaterThanOrEqual(0);
      expect(box.right).toBeLessThanOrEqual(size.width + 1);
      expect(box.bottom).toBeLessThanOrEqual(size.height + 1);
      expect(box.width).toBeGreaterThan(0);
    }
    const hero = await page.locator(".mode-hero").boundingBox();
    const heroHeight = hero?.height ?? 0;
    if (size.height === 700) {
      expect(heroHeight).toBeGreaterThan(previousHeroHeight);
    }
    previousHeroHeight = heroHeight;
  }
});

test("设置页变矮后能用滚轮到达底部，增高后不多余滚动", async ({ page }) => {
  await openSettings(page, "执行");
  await page.setViewportSize({ width: 860, height: 280 });
  const scrollArea = page.locator(".settings-page");
  await expect
    .poll(() =>
      scrollArea.evaluate(
        (element) => element.scrollHeight > element.clientHeight,
      ),
    )
    .toBe(true);
  await scrollArea.hover();
  await page.mouse.wheel(0, 1200);
  await expect
    .poll(() => scrollArea.evaluate((element) => element.scrollTop))
    .toBeGreaterThan(0);
  // 先确认滚轮把控件带入视野，不能依赖 click 的自动滚动。
  const confirmation = page.getByLabel("确认游戏键位", { exact: true });
  await expect(confirmation).toBeInViewport();
  await page.setViewportSize({ width: 860, height: 800 });
  await expect
    .poll(() =>
      scrollArea.evaluate(
        (element) => element.scrollHeight <= element.clientHeight + 1,
      ),
    )
    .toBe(true);
});

test("游戏执行键位可修改并回显，不冒充系统快捷键验收", async ({ page }) => {
  await openSettings(page, "执行");
  const keys = [
    ["暂停键", "Space"],
    ["技能键", "F6"],
    ["撤退键", "F7"],
  ] as const;
  for (const [label, key] of keys) {
    const input = page.getByLabel(label, { exact: true });
    await input.fill(key);
    await input.press("Tab");
  }
  await expect
    .poll(() =>
      page.evaluate(() => window.__smoke.snapshot.settings.retreatKey),
    )
    .toBe("F7");
  const calls = await page.evaluate(() =>
    window.__smoke.calls.filter((call) => call.command === "update_settings"),
  );
  expect(calls.at(-1)?.args.input).toMatchObject({
    pauseKey: "Space",
    skillKey: "F6",
    retreatKey: "F7",
  });
  const confirmation = page.getByLabel("确认游戏键位", { exact: true });
  await expect(confirmation).not.toBeChecked();
  await page.getByRole("button", { name: "返回", exact: true }).click();
  await openSettings(page, "执行");
  for (const [label, key] of keys) {
    await expect(page.getByLabel(label, { exact: true })).toHaveValue(key);
  }
});

test("进入整理页扩高，退出恢复进入前宽高，连续切换不漂移", async ({ page }) => {
  for (const original of [
    { width: 980, height: 330 },
    { width: 1000, height: 600 },
  ]) {
    await page.setViewportSize(original);
    await page.keyboard.press("h");
    await expect(page.getByRole("region", { name: "轴编辑" })).toBeVisible();
    await expect
      .poll(() => page.viewportSize())
      .toEqual({
        width: original.width,
        height: Math.max(460, original.height),
      });
    await page.setViewportSize({ width: 1200, height: 680 });
    await page.getByRole("button", { name: "返回", exact: true }).click();
    await expect.poll(() => page.viewportSize()).toEqual(original);
    const restored = await page.evaluate(() => window.__smoke.sizes.at(-1));
    expect(restored).toEqual(original);
  }
});

test("录屏区段、生成方式、父版本和冲突下拉进入对应请求", async ({ page }) => {
  await page.getByRole("tab", { name: "视频分析", exact: true }).click();
  await page.getByLabel("关卡区段", { exact: true }).selectOption("1");
  await expect(page.locator(".axis-title")).toHaveText("冒烟区段 2");
  await page.getByRole("button", { name: "编辑轴", exact: true }).click();
  await page.getByRole("button", { name: "录屏接续", exact: true }).click();
  await page.getByLabel("校对关卡区段", { exact: true }).selectOption("0");
  await expect(page.getByLabel(/^录屏区段/)).toHaveValue("0");
  await page.getByLabel(/^生成方式/).selectOption("newAxis");
  await expect(page.getByText("创建录屏轴", { exact: true })).toBeVisible();
  await page.getByLabel(/^父版本（只读）/).selectOption("revision-old");
  await page.getByLabel("已核对录屏源锚点与轴目标帧").check();
  await page.getByRole("button", { name: "检查合并", exact: true }).click();
  const conflict = page.locator(".continuation-preview select");
  const create = page.getByRole("button", {
    name: "创建接续版本",
    exact: true,
  });
  await expect(create).toBeDisabled();
  await conflict.selectOption("excludeCandidate");
  await expect(create).toBeEnabled();
  await create.click();
  await expect
    .poll(() =>
      page.evaluate(
        () =>
          window.__smoke.calls.find(
            (call) => call.command === "create_recording_merge_revision",
          )?.args.input,
      ),
    )
    .toMatchObject({
      mode: "newAxis",
      parentRevisionId: "revision-old",
      segmentIndex: 0,
      conflictDecisions: [
        { candidateId: "candidate-smoke", decision: "excludeCandidate" },
      ],
    });
});

test("诊断日志可开启、关闭并导出", async ({ page }) => {
  await openSettings(page, "日志");
  const toggle = page.getByRole("switch", { name: "调试模式" });
  await expect(toggle).not.toBeChecked();
  await page
    .locator(".log-settings")
    .screenshot({ path: ".local/log-settings-dark.png" });
  await page.evaluate(() => {
    document.documentElement.dataset.theme = "light";
  });
  await page
    .locator(".log-settings")
    .screenshot({ path: ".local/log-settings-light.png" });
  await toggle.check();
  await expect(toggle).toBeChecked();
  await page
    .getByRole("button", { name: "导出日志压缩包", exact: false })
    .click();
  await expect(page.getByRole("status")).toHaveText("日志已导出");
  await page.getByRole("button", { name: "历史日志 查看任务执行日志" }).click();
  await expect(page.getByRole("region", { name: "历史日志" })).toContainText(
    "测试历史",
  );
  await page
    .getByRole("button", { name: "错误日志 查看应用异常和错误记录" })
    .click();
  await expect(page.getByRole("region", { name: "错误日志" })).toContainText(
    "测试错误",
  );
  await toggle.uncheck();
  await expect(toggle).not.toBeChecked();
  const calls = await page.evaluate(() =>
    window.__smoke.calls.filter((call) => call.command === "export_logs"),
  );
  expect(calls).toHaveLength(1);
  expect(calls[0]?.args.path).toBe("smoke-output.axis.json");
});
