import { useEffect, useMemo, useState } from "react";
import type {
  RecordingMergeInput,
  RecordingMergePreview,
} from "./generated/bindings";

export type ContinuationRevisionOption = {
  id: string;
  label: string;
  attemptId: string | null;
  createdFrame: number;
  eligible: boolean;
};

export type ContinuationSegmentOption = {
  index: number;
  label: string;
  candidateCount: number;
};

type Props = {
  recordingAnalysisId: string;
  revisions: ContinuationRevisionOption[];
  segments: ContinuationSegmentOption[];
  onPreview: (
    request: RecordingMergeInput,
  ) => Promise<RecordingMergePreview | null>;
  onCreate: (request: RecordingMergeInput) => Promise<boolean>;
};

export default function RecordingContinuation({
  recordingAnalysisId,
  revisions,
  segments,
  onPreview,
  onCreate,
}: Props) {
  const firstEligibleRevisionId =
    revisions.find((revision) => revision.eligible)?.id ?? null;
  const [mode, setMode] = useState<"newAxis" | "continuation">(
    firstEligibleRevisionId ? "continuation" : "newAxis",
  );
  const [revisionId, setRevisionId] = useState(
    firstEligibleRevisionId ?? revisions[0]?.id ?? "",
  );
  const [segmentIndex, setSegmentIndex] = useState(segments[0]?.index ?? 0);
  const [sourceAnchorFrame, setSourceAnchorFrame] = useState(0);
  const [newAxisTargetFrame, setNewAxisTargetFrame] = useState(0);
  const [manualAlignmentConfirmed, setManualAlignmentConfirmed] =
    useState(false);
  const [preview, setPreview] = useState<RecordingMergePreview | null>(null);
  const [decisions, setDecisions] = useState<
    Record<string, "keepCandidate" | "excludeCandidate">
  >({});
  const revision = useMemo(
    () => revisions.find((candidate) => candidate.id === revisionId) ?? null,
    [revisionId, revisions],
  );
  const targetAnchorFrame =
    mode === "newAxis" ? newAxisTargetFrame : (revision?.createdFrame ?? 0);
  const offsetFrames = targetAnchorFrame - sourceAnchorFrame;

  useEffect(() => {
    if (
      mode === "continuation" &&
      !revision?.eligible &&
      firstEligibleRevisionId
    ) {
      setRevisionId(firstEligibleRevisionId);
    }
  }, [firstEligibleRevisionId, mode, revision]);

  useEffect(() => {
    if (
      !segments.some((segment) => segment.index === segmentIndex) &&
      segments[0]
    ) {
      setSegmentIndex(segments[0].index);
    }
  }, [segmentIndex, segments]);

  function request(): RecordingMergeInput {
    return {
      mode,
      parentRevisionId: revisionId,
      recordingAnalysisId,
      segmentIndex,
      sourceAnchorFrame,
      targetAnchorFrame,
      offsetFrames,
      manualAlignmentConfirmed,
      conflictDecisions: Object.entries(decisions).map(
        ([candidateId, decision]) => ({ candidateId, decision }),
      ),
    };
  }

  if (!revisions.length || !segments.length) {
    return (
      <section className="continuation-panel continuation-panel--empty">
        需要一个会话版本和一个已完成分析的录屏区段，才能创建录屏轴。
      </section>
    );
  }

  const conflictsResolved =
    preview?.conflicts.every((conflict) => decisions[conflict.candidateId]) ??
    false;

  return (
    <section className="continuation-panel" aria-label="录屏接续合并">
      <header>
        <div>
          <strong>
            {mode === "newAxis" ? "创建录屏轴" : "接续到本局版本"}
          </strong>
          <span>旧版本保持不变；只合入源锚点后的已确认候选</span>
        </div>
        <label>
          生成方式
          <select
            onChange={(event) => {
              setMode(event.target.value as "newAxis" | "continuation");
              setPreview(null);
            }}
            value={mode}
          >
            <option value="newAxis">新建录屏轴</option>
            <option disabled={!firstEligibleRevisionId} value="continuation">
              接续本局版本
            </option>
          </select>
        </label>
        <label>
          {mode === "newAxis" ? "父版本（只读）" : "接管版本"}
          <select
            onChange={(event) => {
              setRevisionId(event.target.value);
              setPreview(null);
            }}
            value={revisionId}
          >
            {revisions.map((option) => (
              <option
                disabled={mode === "continuation" && !option.eligible}
                key={option.id}
                value={option.id}
              >
                {option.label}
              </option>
            ))}
          </select>
        </label>
        <label>
          录屏区段
          <select
            onChange={(event) => {
              setSegmentIndex(Number(event.target.value));
              setPreview(null);
            }}
            value={segmentIndex}
          >
            {segments.map((segment) => (
              <option key={segment.index} value={segment.index}>
                {segment.label} · {segment.candidateCount} 个候选
              </option>
            ))}
          </select>
        </label>
      </header>

      <div className="continuation-alignment">
        <label>
          录屏接管锚点
          <input
            min="0"
            onChange={(event) => {
              setSourceAnchorFrame(Number(event.target.value));
              setPreview(null);
            }}
            type="number"
            value={sourceAnchorFrame}
          />
        </label>
        <span>
          录屏 F{sourceAnchorFrame} → 轴 F{targetAnchorFrame} · 偏移{" "}
          {offsetFrames >= 0 ? "+" : ""}
          {offsetFrames}
        </span>
        {mode === "newAxis" && (
          <label>
            轴目标帧
            <input
              min="0"
              onChange={(event) => {
                setNewAxisTargetFrame(Number(event.target.value));
                setPreview(null);
              }}
              type="number"
              value={newAxisTargetFrame}
            />
          </label>
        )}
        <label className="continuation-confirmation">
          <input
            checked={manualAlignmentConfirmed}
            onChange={(event) =>
              setManualAlignmentConfirmed(event.target.checked)
            }
            type="checkbox"
          />
          已核对录屏源锚点与轴目标帧
        </label>
        <button
          disabled={
            !revision ||
            (mode === "continuation" && !revision.eligible) ||
            !manualAlignmentConfirmed
          }
          onClick={async () => {
            setDecisions({});
            setPreview(
              await onPreview({ ...request(), conflictDecisions: [] }),
            );
          }}
          type="button"
        >
          检查合并
        </button>
      </div>

      {preview && (
        <div className="continuation-preview">
          <span>
            将合入 {preview.candidateCount} 个候选；源锚点前已排除{" "}
            {preview.skippedBeforeAnchor} 个
          </span>
          {preview.conflicts.map((conflict) => (
            <label key={`${conflict.candidateId}-${conflict.existingEventId}`}>
              F{conflict.alignedFrame} {conflict.kind} · {conflict.tile} 与{" "}
              {conflict.existingEventId} 冲突
              <select
                onChange={(event) =>
                  setDecisions((current) => ({
                    ...current,
                    [conflict.candidateId]: event.target.value as
                      | "keepCandidate"
                      | "excludeCandidate",
                  }))
                }
                value={decisions[conflict.candidateId] ?? ""}
              >
                <option value="">待确认</option>
                <option value="keepCandidate">保留录屏候选</option>
                <option value="excludeCandidate">排除录屏候选</option>
              </select>
            </label>
          ))}
          <button
            className="button--primary"
            disabled={!conflictsResolved}
            onClick={() => onCreate(request())}
            type="button"
          >
            创建接续版本
          </button>
        </div>
      )}
    </section>
  );
}
