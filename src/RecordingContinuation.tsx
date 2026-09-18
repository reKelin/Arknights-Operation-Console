import { useEffect, useMemo, useState } from "react";

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

export type ContinuationConflict = {
  candidateId: string;
  existingEventId: string;
  frame: number;
  operation: string;
};

export type ContinuationPreview = {
  conflicts: ContinuationConflict[];
  skippedBeforeAnchor: number;
};

export type ContinuationRequest = {
  parentRevisionId: string;
  segmentIndex: number;
  sourceAnchorFrame: number;
  targetAnchorFrame: number;
  offsetFrames: number;
  manualAlignmentConfirmed: boolean;
  conflictDecisions: Array<{
    candidateId: string;
    decision: "keepCandidate" | "excludeCandidate";
  }>;
};

type Props = {
  revisions: ContinuationRevisionOption[];
  segments: ContinuationSegmentOption[];
  onPreview: (request: ContinuationRequest) => Promise<ContinuationPreview>;
  onCreate: (request: ContinuationRequest) => Promise<boolean>;
};

export default function RecordingContinuation({
  revisions,
  segments,
  onPreview,
  onCreate,
}: Props) {
  const firstRevision = revisions.find((revision) => revision.eligible) ?? null;
  const [revisionId, setRevisionId] = useState(firstRevision?.id ?? "");
  const [segmentIndex, setSegmentIndex] = useState(segments[0]?.index ?? 0);
  const [sourceAnchorFrame, setSourceAnchorFrame] = useState(0);
  const [manualAlignmentConfirmed, setManualAlignmentConfirmed] =
    useState(false);
  const [preview, setPreview] = useState<ContinuationPreview | null>(null);
  const [decisions, setDecisions] = useState<
    Record<string, "keepCandidate" | "excludeCandidate">
  >({});
  const revision = useMemo(
    () => revisions.find((candidate) => candidate.id === revisionId) ?? null,
    [revisionId, revisions],
  );
  const targetAnchorFrame = revision?.createdFrame ?? 0;
  const offsetFrames = targetAnchorFrame - sourceAnchorFrame;

  useEffect(() => {
    if (!revision?.eligible && firstRevision) setRevisionId(firstRevision.id);
  }, [firstRevision, revision]);

  useEffect(() => {
    if (
      !segments.some((segment) => segment.index === segmentIndex) &&
      segments[0]
    ) {
      setSegmentIndex(segments[0].index);
    }
  }, [segmentIndex, segments]);

  function request(): ContinuationRequest {
    return {
      parentRevisionId: revisionId,
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
        需要一个接管版本和一个已完成分析的录屏区段，才能创建接续版本。
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
          <strong>接续到本局版本</strong>
          <span>父版本保持不变；只合入源锚点后的已确认候选</span>
        </div>
        <label>
          接管版本
          <select
            onChange={(event) => {
              setRevisionId(event.target.value);
              setPreview(null);
            }}
            value={revisionId}
          >
            {revisions.map((option) => (
              <option
                disabled={!option.eligible}
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
          F{sourceAnchorFrame} → 本局 F{targetAnchorFrame} · 偏移{" "}
          {offsetFrames >= 0 ? "+" : ""}
          {offsetFrames}
        </span>
        <label className="continuation-confirmation">
          <input
            checked={manualAlignmentConfirmed}
            onChange={(event) =>
              setManualAlignmentConfirmed(event.target.checked)
            }
            type="checkbox"
          />
          已核对录屏与接管帧
        </label>
        <button
          disabled={!revision?.eligible || !manualAlignmentConfirmed}
          onClick={async () => {
            setDecisions({});
            setPreview(await onPreview(request()));
          }}
          type="button"
        >
          检查合并
        </button>
      </div>

      {preview && (
        <div className="continuation-preview">
          <span>接管前候选已排除 {preview.skippedBeforeAnchor} 个</span>
          {preview.conflicts.map((conflict) => (
            <label key={`${conflict.candidateId}-${conflict.existingEventId}`}>
              F{conflict.frame} {conflict.operation} 与{" "}
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
