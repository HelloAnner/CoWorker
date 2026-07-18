import {
  Bot,
  CheckCircle2,
  CircleAlert,
  LoaderCircle,
  RefreshCw,
  ScanLine,
  Settings2,
  UserRound,
  Waypoints,
  Wrench,
} from "lucide-react";
import { DiagnosticRow } from "../components/DiagnosticRow";
import { useI18n } from "../i18n";
import { diagnosticsForCoworker, enabledCoworkers, normalizeCoworkers, overviewReadiness } from "../lib/bridgeLogic";
import type { BridgeCoworker, BridgeStatus, ConfigValue, DesktopActorId, DiagnosticResult } from "../tauri";

type BootstrapPhase = "loading" | "ready" | "error";
type RouteNodeState = "ok" | "bad" | "pending";

function resultState(result?: DiagnosticResult): RouteNodeState {
  if (!result) return "pending";
  return result.ok ? "ok" : "bad";
}

function linkState(left: RouteNodeState, right: RouteNodeState): RouteNodeState {
  if (left === "bad" || right === "bad") return "bad";
  if (left === "ok" && right === "ok") return "ok";
  return "pending";
}

function compactEndpoint(endpoint: string) {
  if (!endpoint) return "";
  try {
    const url = new URL(endpoint);
    const path = url.pathname === "/" ? "" : url.pathname.replace(/\/$/, "");
    return `${url.host}${path}${url.search}`;
  } catch {
    return endpoint;
  }
}

export function StatusView({
  status,
  bootstrapPhase,
  isDirty,
  diagnostics,
  diagnosticsRunning,
  selectedCoworker,
  config,
  configPath,
  configurationReady,
  onOpenSettings,
  onRefresh,
  onRunDiagnostics,
}: {
  status: BridgeStatus | null;
  bootstrapPhase: BootstrapPhase;
  isDirty: boolean;
  diagnostics: DiagnosticResult[];
  diagnosticsRunning: boolean;
  selectedCoworker?: BridgeCoworker;
  config: ConfigValue;
  configPath: string;
  configurationReady: boolean;
  onOpenSettings: () => void;
  onRefresh: () => void;
  onRunDiagnostics: () => void;
}) {
  const { t } = useI18n();
  const runtimeCoworkers = status?.coworkers ?? [];
  const runtimeCoworker =
    runtimeCoworkers.find((coworker) => coworker.coworker_id === selectedCoworker?.coworker_id) ?? runtimeCoworkers[0];
  const actors = status?.actors ?? [];
  const configuredActors =
    config.actors && typeof config.actors === "object" && !Array.isArray(config.actors)
      ? (config.actors as Record<string, unknown>)
      : {};
  const actorIsEnabled = (actorId: DesktopActorId) => {
    const actorConfig = configuredActors[actorId];
    return !(
      actorConfig
      && typeof actorConfig === "object"
      && !Array.isArray(actorConfig)
      && (actorConfig as Record<string, unknown>).enabled === false
    );
  };
  const enabledActors = actors.filter((actor) => actorIsEnabled(actor.actor_id));
  const availableActorCount = enabledActors.filter((actor) => actor.available).length;
  const readinessStatus = status
    ? { ...status, actors: actors.filter((actor) => actorIsEnabled(actor.actor_id)) }
    : null;
  const runtimeReadiness = overviewReadiness(readinessStatus);
  const readiness = bootstrapPhase === "error"
    ? "failed"
    : runtimeReadiness === "ready" && diagnostics.some((result) => !result.ok)
      ? "partial"
      : runtimeReadiness;
  const codexActor = actors.find((actor) => actor.actor_id === "codex");
  const commandDiag = diagnostics.find((item) => item.name === "Codex command");
  const coworkerDiag = runtimeCoworker ? diagnosticsForCoworker(diagnostics, runtimeCoworker) : undefined;
  const runtimeReady = bootstrapPhase === "ready";
  const showActors = runtimeReady && actors.length > 0;
  const bootstrapLoading = bootstrapPhase === "loading";
  const bootstrapFailed = bootstrapPhase === "error";
  const needsConfiguration = runtimeReady && !configurationReady;
  const configuredCodexId = String(config.codex_id ?? "").trim();
  const configuredCoworkers = enabledCoworkers(normalizeCoworkers(config));
  const configuredCoworker =
    configuredCoworkers.find((coworker) => coworker.coworker_id === selectedCoworker?.coworker_id)
    ?? configuredCoworkers[0];
  const useRuntimeSnapshot = runtimeReady && Boolean(status) && status?.state !== "stopped";
  const routeSource = !runtimeReady ? "unavailable" : useRuntimeSnapshot ? "runtime" : isDirty ? "draft" : "saved";
  const routeCodexId = useRuntimeSnapshot ? status?.codex_id : routeSource === "unavailable" ? null : configuredCodexId;
  const routeCoworker = useRuntimeSnapshot ? runtimeCoworker : routeSource === "unavailable" ? undefined : configuredCoworker;
  const routeEndpoint = routeCoworker?.base_url ?? "";
  const routeConfigValue = routeSource === "unavailable"
    ? "—"
    : routeSource === "draft"
      ? t("status.routeConfigDraft")
      : routeSource === "runtime"
        ? isDirty
          ? t("status.routeConfigSavedRuntime")
          : t("status.routeConfigActive")
        : t("status.routeConfigSaved");
  const routeCoworkerCount = routeSource === "unavailable"
    ? "—"
    : useRuntimeSnapshot
      ? runtimeCoworkers.length
      : configuredCoworkers.length;
  const routeAriaLabel = routeSource === "runtime"
    ? t("status.routeRuntimeLabel")
    : routeSource === "draft"
      ? t("status.routeDraftLabel")
      : routeSource === "saved"
        ? t("status.routeSavedLabel")
        : t("status.routeUnavailableLabel");

  const codexState: RouteNodeState = !runtimeReady
    ? "pending"
    : !actorIsEnabled("codex")
      ? "pending"
      : codexActor
      ? codexActor.available
        ? "ok"
        : "bad"
      : commandDiag
        ? resultState(commandDiag)
        : status?.state === "running" && status.codex_id
          ? "ok"
          : "pending";
  const bridgeState: RouteNodeState = !runtimeReady
    ? "pending"
    : status?.state === "running" && !status.last_error
      ? "ok"
      : status?.state === "stopped"
        ? "pending"
        : "bad";
  const coworkerState: RouteNodeState = !runtimeReady
    ? "pending"
    : runtimeCoworker
      ? coworkerDiag
        ? resultState(coworkerDiag)
        : status?.state === "running"
          ? "ok"
          : "pending"
      : "pending";

  const failedDiagnostics = diagnostics.filter((result) => !result.ok);
  const passedDiagnostics = diagnostics.filter((result) => result.ok);
  const diagnosticsDisabled = bootstrapPhase !== "ready" || isDirty || diagnosticsRunning;
  const diagnosticSummary = bootstrapFailed
    ? t("status.statusUnavailable")
    : bootstrapLoading
      ? t("status.statusLoading")
      : needsConfiguration
        ? t("status.configurationNeeded")
        : diagnosticsRunning
          ? t("status.diagnosticsRunning")
          : diagnostics.length === 0
            ? t("status.diagnosticsSummaryIdle")
            : failedDiagnostics.length > 0
              ? t("status.diagnosticsIssues", { count: failedDiagnostics.length })
              : t("status.diagnosticsSummary", { passed: passedDiagnostics.length, total: diagnostics.length });
  const diagnosticSummaryClass = failedDiagnostics.length > 0
    ? "diagnosticsSummary attention"
    : diagnostics.length > 0
      ? "diagnosticsSummary success"
      : "diagnosticsSummary";
  const actorSummaryState = enabledActors.length === 0
    ? "disabled"
    : availableActorCount === enabledActors.length
      ? "ok"
      : "attention";
  const actorSummary = enabledActors.length === 0
    ? t("status.actorAllDisabled")
    : t("status.actorSummary", { available: availableActorCount, total: enabledActors.length });

  const actorLabel = (actorId: DesktopActorId) => {
    if (actorId === "local") return t("actors.local");
    if (actorId === "claude") return t("actors.claude");
    return t("actors.codex");
  };

  const diagnosticLabel = (name: string) => {
    if (name === "Desktop transport security") return t("status.diagTransportSecurity");
    if (name === "Codex command") return t("status.diagCodexCommand");
    if (name === "Codex app-server") return t("status.diagCodexAppServer");
    if (name === "Claude Code") return t("status.diagClaudeCode");
    if (name === "Claude MCP sidecar") return t("status.diagClaudeSidecar");
    return name.startsWith("Coworker ") ? name.slice("Coworker ".length) : name;
  };

  return (
    <div className="contentGrid statusDashboard">
      <section className="panel bridgeOverview" data-readiness={readiness} aria-labelledby="overview-readiness-title">
        <div className="statusHero">
          <p className="eyebrow">{t("status.routeEyebrow")}</p>
          <div className="statusTitleLine">
            <h3 id="overview-readiness-title">{t(`status.readiness.${readiness}.label`)}</h3>
          </div>
          <p>{t(`status.readiness.${readiness}.desc`)}</p>
        </div>

        {runtimeReady ? (
          <div className="routeCanvas" data-source={routeSource}>
            <div className="routeMap" aria-label={routeAriaLabel}>
            <div className="routeNode" data-state={codexState}>
              <span className="routeNodeIcon" aria-hidden="true"><Bot size={16} /></span>
              <span>
                <small>{t("common.codex")}</small>
                <strong>{runtimeReady ? routeCodexId || t("common.notConfigured") : "—"}</strong>
              </span>
            </div>
            <i className="routeLink" data-state={linkState(codexState, bridgeState)} aria-hidden="true" />
            <div className="routeNode routeNodeBridge" data-state={bridgeState}>
              <span className="routeNodeIcon" aria-hidden="true"><Waypoints size={16} /></span>
              <span>
                <small>{t("status.routeBridge")}</small>
                <strong>{runtimeReady && status ? t(`runtimeState.${status.state}`) : "—"}</strong>
              </span>
            </div>
            <i className="routeLink" data-state={linkState(bridgeState, coworkerState)} aria-hidden="true" />
            <div className="routeNode" data-state={coworkerState}>
              <span className="routeNodeIcon" aria-hidden="true"><UserRound size={16} /></span>
              <span>
                <small>{t("status.routeCoworker")}</small>
                <strong>{runtimeReady ? routeCoworker?.display_name || t("status.noRuntimeCoworker") : "—"}</strong>
              </span>
            </div>
            </div>

            <dl className="routeFacts">
            <div className="routeFactEndpoint">
              <dt>{t("status.routeEndpoint")}</dt>
              <dd title={routeEndpoint || undefined}>
                {runtimeReady ? compactEndpoint(routeEndpoint) || t("common.notConfigured") : "—"}
              </dd>
            </div>
            <div className="routeFactConfig">
              <dt>{t("status.routeConfig")}</dt>
              <dd title={routeSource === "runtime" ? status?.config_path ?? undefined : routeSource === "saved" ? configPath : undefined}>
                {routeConfigValue}
              </dd>
            </div>
            <div className="routeFactCount">
              <dt>{t(useRuntimeSnapshot ? "status.routeOnlineCoworkers" : "status.routeConfiguredCoworkers")}</dt>
              <dd>{routeCoworkerCount}</dd>
            </div>
            </dl>
          </div>
        ) : (
          <div className="routeStandby" data-state={bootstrapFailed ? "error" : "loading"}>
            <span className="routeStandbyIcon" aria-hidden="true">
              {bootstrapLoading ? <LoaderCircle className="spinIcon" size={18} /> : <CircleAlert size={18} />}
            </span>
            <span>
              <strong>{t(bootstrapFailed ? "status.routeErrorTitle" : "status.routeLoadingTitle")}</strong>
              <small>{t(bootstrapFailed ? "status.routeErrorDesc" : "status.routeLoadingDesc")}</small>
            </span>
          </div>
        )}
      </section>

      {showActors && (
        <section className="panel actorStatusPanel">
          <div className="sectionHead">
            <h3>{t("status.actorTitle")}</h3>
            <span className="actorPanelSummary" data-state={actorSummaryState}>{actorSummary}</span>
          </div>
          <div className="actorStatusList">
            {actors.map((actor) => {
              const enabled = actorIsEnabled(actor.actor_id);
              const actorState = !enabled ? "disabled" : actor.available ? "ok" : "bad";
              const actorDetail = enabled && !actor.available
                ? actor.message || t("status.actorNotAvailable")
                : "";
              return (
                <div className="actorStatusRow" data-state={actorState} key={actor.actor_id}>
                  <span className="actorStatusDot" aria-hidden="true" />
                  <span>
                    <strong>{actorLabel(actor.actor_id)}</strong>
                    {actorDetail && <small>{actorDetail}</small>}
                  </span>
                  <em>
                    {!enabled
                      ? t("status.actorDisabled")
                      : actor.available
                        ? t("status.actorAvailable")
                        : t("status.actorNotAvailable")}
                  </em>
                </div>
              );
            })}
          </div>
        </section>
      )}

      <section className={showActors ? "panel healthPanel" : "panel healthPanel healthPanelWide"} aria-busy={diagnosticsRunning}>
        <div className="sectionHead healthPanelHead">
          <div>
            <div className="diagnosticsTitleLine">
              <h3>
                {bootstrapFailed
                  ? t("status.statusIssueTitle")
                  : bootstrapLoading
                    ? t("status.statusLoadingTitle")
                    : needsConfiguration
                      ? t("status.attentionTitle")
                      : t("status.healthCheckTitle")}
              </h3>
              <span className={diagnosticSummaryClass}>
                {diagnosticSummary}
              </span>
            </div>
          </div>
          {bootstrapFailed ? (
            <button className="softButton" onClick={onRefresh} type="button">
              <RefreshCw size={16} /> {t("status.retry")}
            </button>
          ) : bootstrapLoading ? (
            <button className="softButton" disabled type="button">
              <LoaderCircle className="spinIcon" size={16} /> {t("status.statusLoading")}
            </button>
          ) : needsConfiguration ? (
            <button className="softButton" onClick={onOpenSettings} type="button">
              <Settings2 size={16} /> {t("status.openSettings")}
            </button>
          ) : (
            <button
              className="softButton"
              onClick={onRunDiagnostics}
              disabled={diagnosticsDisabled}
              title={isDirty ? t("status.diagnosticsDirty") : t("status.runDiagnostics")}
              type="button"
            >
              {diagnosticsRunning ? <LoaderCircle className="spinIcon" size={16} /> : <Wrench size={16} />}
              {diagnosticsRunning ? t("status.diagnosticsRunning") : t("status.runDiagnostics")}
            </button>
          )}
        </div>

        {bootstrapFailed ? (
          <div className="diagnosticsEmpty diagnosticsGuard">
            <CircleAlert size={18} aria-hidden="true" />
            <p>{t("status.statusUnavailableEmpty")}</p>
          </div>
        ) : bootstrapLoading ? (
          <div className="diagnosticsEmpty" role="status">
            <LoaderCircle className="spinIcon" size={18} aria-hidden="true" />
            <p>{t("status.statusLoadingEmpty")}</p>
          </div>
        ) : needsConfiguration ? (
          <div className="diagnosticsEmpty diagnosticsSetup">
            <Settings2 size={18} aria-hidden="true" />
            <p>{isDirty ? t("status.runtimeUsingSavedConfig") : t("status.setupEmpty")}</p>
          </div>
        ) : isDirty ? (
          <div className="diagnosticsEmpty diagnosticsGuard">
            <CircleAlert size={18} aria-hidden="true" />
            <p>{t("status.diagnosticsDirty")}</p>
          </div>
        ) : diagnosticsRunning ? (
          <div className="diagnosticsEmpty" role="status">
            <LoaderCircle className="spinIcon" size={18} aria-hidden="true" />
            <p>{t("status.diagnosticsRunning")}</p>
          </div>
        ) : diagnostics.length === 0 ? (
          <div className="diagnosticsEmpty">
            <ScanLine size={19} aria-hidden="true" />
            <p>{t("status.diagnosticsEmpty")}</p>
          </div>
        ) : (
          <div className="diagnosticResults">
            {failedDiagnostics.length > 0 && (
              <div className="diagnostics diagnosticsFailures">
                {failedDiagnostics.map((result, index) => (
                  <DiagnosticRow key={`${result.name}-${index}`} label={diagnosticLabel(result.name)} result={result} />
                ))}
              </div>
            )}
            {passedDiagnostics.length > 0 && (
              <details className="passedChecks">
                <summary>
                  <CheckCircle2 size={16} aria-hidden="true" />
                  <span>{t("status.passedChecks", { count: passedDiagnostics.length })}</span>
                </summary>
                <div className="diagnostics">
                  {passedDiagnostics.map((result, index) => (
                    <DiagnosticRow key={`${result.name}-${index}`} label={diagnosticLabel(result.name)} result={result} />
                  ))}
                </div>
              </details>
            )}
          </div>
        )}
      </section>
    </div>
  );
}
