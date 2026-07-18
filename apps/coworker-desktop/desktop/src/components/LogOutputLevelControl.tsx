import { useI18n } from "../i18n";
import type { DictKey } from "../i18n/en";
import type { LogOutputLevel } from "../tauri";

export const logOutputLevels: LogOutputLevel[] = ["ERROR", "WARN", "INFO", "DEBUG", "TRACE"];

export function normalizeLogOutputLevel(value: unknown): LogOutputLevel {
  const normalized = String(value ?? "").trim().toUpperCase();
  return logOutputLevels.includes(normalized as LogOutputLevel) ? (normalized as LogOutputLevel) : "INFO";
}

export function LogOutputLevelControl({
  value,
  onChange,
  labelledBy,
  ariaLabel,
  compact = false,
  disabled = false,
}: {
  value: unknown;
  onChange: (value: LogOutputLevel) => void;
  labelledBy?: string;
  ariaLabel?: string;
  compact?: boolean;
  disabled?: boolean;
}) {
  const { t } = useI18n();
  const selectedLevel = normalizeLogOutputLevel(value);

  return (
    <div
      className={compact ? "logLevelControl compact" : "logLevelControl"}
      role="radiogroup"
      aria-labelledby={labelledBy}
      aria-label={labelledBy ? undefined : ariaLabel}
    >
      {logOutputLevels.map((level) => {
        const active = level === selectedLevel;
        return (
          <button
            key={level}
            className={active ? "active" : ""}
            data-level={level.toLowerCase()}
            type="button"
            role="radio"
            aria-checked={active}
            disabled={disabled}
            onClick={() => onChange(level)}
          >
            <span className="logLevelDot" aria-hidden="true" />
            {t(`logLevel.${level.toLowerCase()}` as DictKey)}
          </button>
        );
      })}
    </div>
  );
}
