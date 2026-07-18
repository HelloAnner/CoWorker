import { Activity, AlertCircle, ArrowDownToLine, Circle, RefreshCw, XCircle } from "lucide-react";
import type { RefObject } from "react";
import { LogOutputLevelControl } from "../components/LogOutputLevelControl";
import { useI18n } from "../i18n";
import type { DictKey } from "../i18n/en";
import { logLevelValues, type LogEntryViewModel, type LogLevel, type LogLevelFilter } from "../lib/bridgeLogic";
import type { LogOutputLevel } from "../tauri";

export function LogsView({
  ledgerRef,
  logParsePending,
  logEntries,
  filteredLogEntries,
  renderedLogEntries,
  hiddenLogEntryCount,
  logLevelCounts,
  logLevelFilter,
  setLogLevelFilter,
  liveLogs,
  setLiveLogs,
  followLatest,
  setFollowLatest,
  logOutputLevel,
  logLevelUpdating,
  setLogOutputLevel,
  onRefreshLog,
  onSelectEntry,
}: {
  ledgerRef: RefObject<HTMLDivElement | null>;
  logParsePending: boolean;
  logEntries: LogEntryViewModel[];
  filteredLogEntries: LogEntryViewModel[];
  renderedLogEntries: LogEntryViewModel[];
  hiddenLogEntryCount: number;
  logLevelCounts: Record<LogLevel, number>;
  logLevelFilter: LogLevelFilter;
  setLogLevelFilter: (value: LogLevelFilter) => void;
  liveLogs: boolean;
  setLiveLogs: (updater: (current: boolean) => boolean) => void;
  followLatest: boolean;
  setFollowLatest: (updater: (current: boolean) => boolean) => void;
  logOutputLevel: unknown;
  logLevelUpdating: boolean;
  setLogOutputLevel: (value: LogOutputLevel) => void;
  onRefreshLog: () => void;
  onSelectEntry: (entry: LogEntryViewModel) => void;
}) {
  const { t } = useI18n();

  return (
    <section className="panel logPanel">
      <div className="sectionHead">
        <div>
          <p className="eyebrow">{t("logs.eyebrow")}</p>
          <h3>{t("logs.title")}</h3>
        </div>
        <div className="logActions">
            <button
              className={liveLogs ? "softButton followButton active" : "softButton followButton"}
              onClick={() => setLiveLogs((current) => !current)}
              aria-pressed={liveLogs}
              title={liveLogs ? t("logs.liveLogsOn") : t("logs.liveLogsOff")}
              type="button"
            >
              <Activity size={14} /> {t("logs.liveLogs")}
            </button>
            <button
              className={followLatest ? "softButton followButton active" : "softButton followButton"}
              onClick={() => setFollowLatest((current) => !current)}
              aria-pressed={followLatest}
              title={followLatest ? t("logs.followLatestOn") : t("logs.followLatestOff")}
              type="button"
            >
              <ArrowDownToLine size={14} /> {t("logs.followLatest")}
            </button>
            <button className="softButton" onClick={onRefreshLog}>
              <RefreshCw size={14} /> {t("logs.refreshLog")}
            </button>
        </div>
      </div>
      <div className="logControlBar">
        <div className="logRuntimeSetting" aria-busy={logLevelUpdating}>
          <span id="runtime-log-level-label">{t("logs.outputLevel")}</span>
          <LogOutputLevelControl
            compact
            value={logOutputLevel}
            labelledBy="runtime-log-level-label"
            disabled={logLevelUpdating}
            onChange={setLogOutputLevel}
          />
          <small>{t("logs.appliesImmediately")}</small>
        </div>
        <div className="logDisplayFilter">
          <span>{t("logs.displayFilter")}</span>
          <div className="levelFilter" role="radiogroup" aria-label={t("aria.filterLogsByLevel")}>
            {logLevelValues.map((value) => {
              const count = value === "all" ? logEntries.length : logLevelCounts[value as LogLevel];
              return (
                <button
                  className={logLevelFilter === value ? "active" : ""}
                  key={value}
                  onClick={() => setLogLevelFilter(value)}
                  role="radio"
                  aria-checked={logLevelFilter === value}
                  title={t("logs.showLevelLogs", { level: t(`logLevel.${value}` as DictKey) })}
                  type="button"
                >
                  {t(`logLevel.${value}` as DictKey)}
                  <span>{count}</span>
                </button>
              );
            })}
          </div>
        </div>
      </div>
      <div className="ledger" ref={ledgerRef}>
        {logParsePending && !logEntries.length ? (
          <div className="emptyLedger">{t("logs.preparingEntries")}</div>
        ) : logEntries.length ? (
          filteredLogEntries.length ? (
            <>
              {hiddenLogEntryCount > 0 && (
                <div className="ledgerWindowNotice">
                  {t("logs.showingLatestOfTotal", { shown: renderedLogEntries.length, total: filteredLogEntries.length })}
                </div>
              )}
              {renderedLogEntries.map((entry) => (
                <article
                  className={`ledgerRow level-${entry.level}`}
                  key={entry.id}
                  role="button"
                  tabIndex={0}
                  title={entry.message}
                  onClick={() => onSelectEntry(entry)}
                  onKeyDown={(event) => {
                    if (event.key === "Enter" || event.key === " ") {
                      event.preventDefault();
                      onSelectEntry(entry);
                    }
                  }}
                >
                  <span className="ledgerRowTime" title={entry.time || t("logs.noTimestamp")}>
                    {entry.timeClock || "--:--"}
                  </span>
                  <span className="ledgerRowIcon" aria-hidden="true">
                    {entry.level === "error" ? <XCircle size={13} /> : entry.level === "warn" ? <AlertCircle size={13} /> : <Circle size={9} />}
                  </span>
                  <span className="ledgerRowTarget">{entry.target}</span>
                  <span className="ledgerRowLevel">{entry.level}</span>
                  {entry.source && (
                    <code className="ledgerRowSource" title={entry.source}>
                      {entry.source}
                    </code>
                  )}
                  <span className="ledgerRowMessage" title={entry.message}>
                    {entry.message}
                  </span>
                </article>
              ))}
            </>
          ) : (
            <div className="emptyLedger">{t("logs.noLevelEntries", { level: t(`logLevel.${logLevelFilter}` as DictKey) })}</div>
          )
        ) : (
          <div className="emptyLedger">{t("logs.noOutputYet")}</div>
        )}
      </div>
    </section>
  );
}
