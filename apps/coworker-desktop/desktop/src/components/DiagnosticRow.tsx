import { CheckCircle2, Circle, XCircle } from "lucide-react";
import type { DiagnosticResult } from "../tauri";
import { useI18n } from "../i18n";

export function DiagnosticRow({ label, result }: { label: string; result?: DiagnosticResult }) {
  const { t } = useI18n();
  const state = result ? (result.ok ? "ok" : "bad") : "pending";
  return (
    <div className={`diag ${state}`}>
      {state === "ok" ? <CheckCircle2 size={16} /> : state === "bad" ? <XCircle size={16} /> : <Circle size={12} />}
      <div>
        <strong>{label}</strong>
        <p>{result?.message ?? t("status.diagNotCheckedYet")}</p>
      </div>
    </div>
  );
}
