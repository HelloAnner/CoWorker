import { Copy, X } from "lucide-react";
import { useI18n } from "../i18n";
import type { LogEntryViewModel } from "../lib/bridgeLogic";

export function LogDetailDialog({
  entry,
  onClose,
  onCopied,
}: {
  entry: LogEntryViewModel;
  onClose: () => void;
  onCopied: (success: boolean) => void;
}) {
  const { t } = useI18n();

  async function copyText() {
    try {
      await navigator.clipboard.writeText(entry.text);
      onCopied(true);
    } catch {
      onCopied(false);
    }
  }

  return (
    <div className="modalBackdrop" role="presentation" onClick={onClose}>
      <div
        className="logDetailDialog"
        role="dialog"
        aria-modal="true"
        aria-label={t("aria.logDetail")}
        onClick={(event) => event.stopPropagation()}
      >
        <div className="sectionHead">
          <div>
            <p className="eyebrow">{t("logs.eyebrow")}</p>
            <h3>{t("logs.detailTitle")}</h3>
          </div>
          <button className="iconButton" onClick={onClose} aria-label={t("aria.closeLogDetail")} type="button">
            <X size={15} />
          </button>
        </div>
        <dl className="logDetailMeta">
          <div>
            <dt>{t("logs.detailTime")}</dt>
            <dd>{entry.time || "--"}</dd>
          </div>
          <div>
            <dt>{t("logs.detailLevel")}</dt>
            <dd>{entry.level}</dd>
          </div>
          <div>
            <dt>{t("logs.detailTarget")}</dt>
            <dd>{entry.target}</dd>
          </div>
          {entry.source && (
            <div>
              <dt>{t("logs.detailSource")}</dt>
              <dd>{entry.source}</dd>
            </div>
          )}
        </dl>
        <div className="logDetailTextWrap">
          <p className="logDetailTextLabel">{t("logs.detailRawText")}</p>
          <pre className="logDetailText">{entry.text}</pre>
        </div>
        <div className="logDetailActions">
          <button className="softButton" onClick={copyText} type="button">
            <Copy size={14} /> {t("logs.copy")}
          </button>
        </div>
      </div>
    </div>
  );
}
