import { useMemo, useState } from "react";
import type {
  AnalysisCandidate,
  CandidateActionKind,
  CandidateConfirmation,
  DraftEvent,
  FacingDirection,
} from "./generated/bindings";

const EVIDENCE_LABELS = {
  deploymentGesture: "检测到部署手势",
  selectedUnitInteraction: "检测到操作区间",
  interruptedInteraction: "检测到未完成的操作区间",
} as const;

const KIND_LABELS: Record<CandidateActionKind, string> = {
  deploy: "部署",
  skill: "技能",
  retreat: "撤退",
};

type Props = {
  candidates: AnalysisCandidate[];
  events: DraftEvent[];
  segmentIndex: number;
  onConfirm: (input: CandidateConfirmation) => Promise<boolean>;
  onPreview: (frame: number) => void;
};

export default function RecordingCandidates({
  candidates,
  events,
  segmentIndex,
  onConfirm,
  onPreview,
}: Props) {
  const visible = useMemo(
    () =>
      candidates.filter((candidate) => candidate.segmentIndex === segmentIndex),
    [candidates, segmentIndex],
  );
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [kind, setKind] = useState<CandidateActionKind | "">("");
  const [frame, setFrame] = useState(0);
  const [operator, setOperator] = useState("");
  const [tile, setTile] = useState("");
  const [direction, setDirection] = useState<FacingDirection>("right");
  const [timeConfirmed, setTimeConfirmed] = useState(false);
  const selected =
    visible.find((candidate) => candidate.id === selectedId) ?? null;
  const convertedIds = useMemo(
    () => new Set(events.flatMap((event) => event.sourceCandidateId ?? [])),
    [events],
  );

  function choose(candidate: AnalysisCandidate) {
    setSelectedId(candidate.id);
    setKind(candidate.kind ?? "");
    setFrame(candidate.gameFrameRange.start);
    setOperator(candidate.operator ?? "");
    setTile(candidate.tile ?? "");
    setDirection(candidate.direction ?? "right");
    setTimeConfirmed(false);
    onPreview(candidate.gameFrameRange.start);
  }

  if (!visible.length) {
    return (
      <section className="recording-candidates recording-candidates--empty">
        当前区段没有可复用的操作状态变化。录屏校对不会凭空补写技能、撤退或格子参数。
      </section>
    );
  }

  const exactTrustedTime =
    selected?.clockQuality === "trusted" &&
    selected.gameFrameRange.start === selected.gameFrameRange.end &&
    frame === selected.gameFrameRange.start;

  return (
    <section className="recording-candidates" aria-label="录屏操作候选">
      <div className="candidate-list">
        {visible.map((candidate) => {
          const converted = convertedIds.has(candidate.id);
          return (
            <button
              className={
                candidate.id === selectedId
                  ? "candidate-row selected"
                  : "candidate-row"
              }
              disabled={converted}
              key={candidate.id}
              onClick={() => choose(candidate)}
              type="button"
            >
              <strong>{EVIDENCE_LABELS[candidate.evidence]}</strong>
              <span>
                {candidate.kind ? KIND_LABELS[candidate.kind] : "类型待确认"} ·
                F{candidate.gameFrameRange.start}–{candidate.gameFrameRange.end}{" "}
                · 置信度 {candidate.confidence}%
              </span>
              <small>
                {converted
                  ? "已加入轴"
                  : `源 PTS ${candidate.sourceStart.rawPts} · 参数待校对`}
              </small>
            </button>
          );
        })}
      </div>

      {selected ? (
        <form
          className="candidate-confirm"
          onSubmit={async (event) => {
            event.preventDefault();
            if (!kind) return;
            const confirmed = await onConfirm({
              candidateId: selected.id,
              kind,
              gameFrame: frame,
              operator: kind === "deploy" ? operator.trim() || null : null,
              tile: tile.trim() || null,
              direction: kind === "deploy" ? direction : null,
              manualTimeConfirmation: exactTrustedTime || timeConfirmed,
            });
            if (confirmed) setSelectedId(null);
          }}
        >
          <label>
            类型
            <select
              onChange={(event) =>
                setKind(event.target.value as CandidateActionKind | "")
              }
              required
              value={kind}
            >
              <option value="">待确认</option>
              <option value="deploy">部署</option>
              <option value="skill">技能</option>
              <option value="retreat">撤退</option>
            </select>
          </label>
          <label>
            帧
            <input
              min="0"
              onChange={(event) => {
                const value = Number(event.target.value);
                setFrame(value);
                onPreview(value);
              }}
              required
              type="number"
              value={frame}
            />
          </label>
          {kind === "deploy" && (
            <label>
              干员 ID
              <input
                onChange={(event) => setOperator(event.target.value)}
                required
                value={operator}
              />
            </label>
          )}
          <label>
            格子
            <input
              onChange={(event) => setTile(event.target.value.toUpperCase())}
              required
              value={tile}
            />
          </label>
          {kind === "deploy" && (
            <label>
              朝向
              <select
                onChange={(event) =>
                  setDirection(event.target.value as FacingDirection)
                }
                value={direction}
              >
                <option value="up">上</option>
                <option value="right">右</option>
                <option value="down">下</option>
                <option value="left">左</option>
              </select>
            </label>
          )}
          {!exactTrustedTime && (
            <label className="candidate-time-confirmation">
              <input
                checked={timeConfirmed}
                onChange={(event) => setTimeConfirmed(event.target.checked)}
                required
                type="checkbox"
              />
              已核对操作时间
            </label>
          )}
          <button className="button--primary" disabled={!kind} type="submit">
            确认并加入轴
          </button>
        </form>
      ) : (
        <p className="candidate-help">
          选择候选后补齐类型、格子和时间；未知参数不会自动推断。
        </p>
      )}
    </section>
  );
}
