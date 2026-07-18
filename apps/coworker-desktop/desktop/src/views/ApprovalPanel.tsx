import { ChevronDown, ShieldAlert, ShieldCheck, ShieldOff } from "lucide-react";
import { useCallback, useEffect, useRef, useState } from "react";
import { useI18n } from "../i18n";
import { riskLevel, summarizeApproval } from "../lib/approvalSummaries";
import type { RiskLevel } from "../lib/approvalSummaries";
import type { DesktopApproval } from "../tauri";

type Props = {
  approval: DesktopApproval;
  busy: boolean;
  /** Number of additional approvals queued after this one. */
  queueCount: number;
  onResolve: (response: {
    behavior: "allow" | "deny";
    updatedInput?: unknown;
    message?: string;
  }) => void;
};

/** Seconds before expiry to switch to urgent (red) display. */
const URGENT_THRESHOLD_S = 30;

function formatRemaining(expiresAt: string): { text: string; urgent: boolean } {
  const remaining = Math.max(0, Math.floor((new Date(expiresAt).getTime() - Date.now()) / 1000));
  if (remaining <= 0) return { text: "", urgent: true };
  const min = Math.floor(remaining / 60);
  const sec = remaining % 60;
  const text = min > 0 ? `${min}:${String(sec).padStart(2, "0")}` : `${sec}s`;
  return { text, urgent: remaining <= URGENT_THRESHOLD_S };
}

const RISK_ICON: Record<RiskLevel, { icon: typeof ShieldAlert; cls: string }> = {
  high: { icon: ShieldOff, cls: "approvalIcon--high" },
  medium: { icon: ShieldAlert, cls: "approvalIcon--medium" },
  low: { icon: ShieldCheck, cls: "approvalIcon--low" },
};

export function ApprovalPanel({ approval, busy, queueCount, onResolve }: Props) {
  const { t } = useI18n();
  const [inputText, setInputText] = useState(() => JSON.stringify(approval.input, null, 2));
  const [inputError, setInputError] = useState("");
  const [denyReason, setDenyReason] = useState("");
  const [showDenyReason, setShowDenyReason] = useState(false);
  const [remaining, setRemaining] = useState(() => formatRemaining(approval.expires_at));
  const denyReasonRef = useRef<HTMLInputElement | null>(null);

  const risk = riskLevel(approval.tool_name, approval.input);
  const summary = summarizeApproval(approval.tool_name, approval.input);
  const { icon: RiskIcon, cls: iconCls } = RISK_ICON[risk];

  // Reset state when the approval changes (queue advances)
  useEffect(() => {
    setInputText(JSON.stringify(approval.input, null, 2));
    setInputError("");
    setDenyReason("");
    setShowDenyReason(false);
    setRemaining(formatRemaining(approval.expires_at));
  }, [approval.request_id, approval.expires_at]);

  // Countdown timer
  useEffect(() => {
    const timer = window.setInterval(() => {
      const next = formatRemaining(approval.expires_at);
      setRemaining(next);
      // When expired, resolve as deny (timeout) — the parent component
      // handles the actual removal via the expired event.
      if (next.text === "" && remaining.text !== "") {
        onResolve({ behavior: "deny", message: "Approval request timed out" });
      }
    }, 1000);
    return () => window.clearInterval(timer);
  }, [approval.expires_at, approval.request_id, onResolve, remaining.text]);

  const allow = useCallback(() => {
    try {
      const updatedInput = JSON.parse(inputText);
      setInputError("");
      onResolve({ behavior: "allow", updatedInput });
    } catch {
      setInputError(t("approval.invalidJson"));
    }
  }, [inputText, onResolve, t]);

  const handleDeny = useCallback(() => {
    if (!showDenyReason) {
      setShowDenyReason(true);
      // Focus the reason input on next frame
      requestAnimationFrame(() => denyReasonRef.current?.focus());
      return;
    }
    onResolve({
      behavior: "deny",
      message: denyReason || "Denied in CoWorker Desktop",
    });
  }, [showDenyReason, denyReason, onResolve]);

  return (
    <div className="approvalBackdrop">
      <aside
        className="approvalPanel"
        role="dialog"
        aria-label={t("approval.dialogLabel")}
      >
        <header className="approvalHeader">
          <span className={`approvalIcon ${iconCls}`} aria-hidden="true">
            <RiskIcon size={19} />
          </span>
          <div>
            <strong>{t("approval.title")}</strong>
            <span className="approvalSubtitle">
              {t("approval.subtitle", {
                tool: approval.tool_name,
                coworker: approval.coworker_id,
              })}
            </span>
          </div>
          {remaining.text && (
            <span className={`approvalCountdown ${remaining.urgent ? "approvalCountdown--urgent" : ""}`}>
              {remaining.text}
            </span>
          )}
        </header>

        {/* Operation summary */}
        {summary !== approval.tool_name && (
          <div className="approvalSummary">{summary}</div>
        )}

        {/* Queue indicator */}
        {queueCount > 0 && (
          <div className="approvalQueue">
            {t("approval.queueCount", { count: queueCount })}
          </div>
        )}

        <details className="approvalDetails" open>
          <summary>
            <ChevronDown size={14} aria-hidden="true" /> {t("approval.reviewInput")}
          </summary>
          <textarea
            aria-label={t("approval.inputLabel")}
            value={inputText}
            onChange={(event) => setInputText(event.target.value)}
          />
          {inputError && <small role="alert">{inputError}</small>}
        </details>

        {/* Deny reason (shown after first Deny click) */}
        {showDenyReason && (
          <div className="approvalDenyReason">
            <input
              ref={denyReasonRef}
              type="text"
              placeholder={t("approval.denyReasonPlaceholder")}
              value={denyReason}
              onChange={(event) => setDenyReason(event.target.value)}
              onKeyDown={(event) => {
                if (event.key === "Enter") {
                  event.preventDefault();
                  handleDeny();
                }
              }}
            />
          </div>
        )}

        <div className="approvalActions">
          <button
            className="dangerAction"
            disabled={busy}
            onClick={handleDeny}
          >
            {showDenyReason ? t("approval.denyWithReason") : t("approval.deny")}
          </button>
          <button
            className="primaryAction"
            disabled={busy}
            onClick={allow}
          >
            {t("approval.allowOnce")}
          </button>
        </div>
      </aside>
    </div>
  );
}
