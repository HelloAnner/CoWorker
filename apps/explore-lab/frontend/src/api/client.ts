import type {
  Branch,
  BranchState,
  CompareResult,
  DiffResult,
  ExperimentDetail,
  ExperimentSummary,
  Scenario,
  ScenarioEvent,
  StepNResult,
  StepResult,
  SystemPromptSnapshot,
} from './types';

const ORCHESTRATOR_BASE = (import.meta.env.VITE_ORCHESTRATOR_BASE_URL || window.location.origin).replace(
  /\/$/,
  ''
);

async function requestJson<T>(base: string, path: string, init?: RequestInit): Promise<T> {
  const response = await fetch(`${base}${path}`, init);
  if (!response.ok) {
    const detail = await response
      .json()
      .then(body => body.detail?.message || body.detail || body.message)
      .catch(() => response.statusText);
    throw new Error(typeof detail === 'string' ? detail : `请求失败：${response.status}`);
  }
  if (response.status === 204) return undefined as T;
  return response.json() as Promise<T>;
}

function orchestrator<T>(path: string, init?: RequestInit): Promise<T> {
  return requestJson<T>(ORCHESTRATOR_BASE, path, init);
}

function jsonInit(method: string, body?: unknown): RequestInit {
  return {
    method,
    headers: { 'Content-Type': 'application/json' },
    body: body === undefined ? undefined : JSON.stringify(body),
  };
}

// ---- Orchestrator：实验 / 分支 / Scenario 管理 ----

export function importExperiment(payload: { coworker_base_url: string; admin_token: string }) {
  return orchestrator<{ experiment_id: string; branch_id: string; control_port: number; status: string }>(
    '/experiments/import',
    jsonInit('POST', payload)
  );
}

export function listExperiments() {
  return orchestrator<{ experiments: ExperimentSummary[] }>('/experiments');
}

export function getExperiment(id: string) {
  return orchestrator<ExperimentDetail>(`/experiments/${id}`);
}

export function createScenario(experimentId: string, name: string, events: ScenarioEvent[]) {
  return orchestrator<Scenario>(`/experiments/${experimentId}/scenarios`, jsonInit('POST', { name, events }));
}

export function listScenarios(experimentId: string) {
  return orchestrator<{ scenarios: Scenario[] }>(`/experiments/${experimentId}/scenarios`);
}

export function listBranches() {
  return orchestrator<{ branches: Branch[] }>('/branches');
}

export function getBranch(id: string) {
  return orchestrator<Branch>(`/branches/${id}`);
}

export function wakeBranch(id: string) {
  return orchestrator<Branch>(`/branches/${id}/wake`, jsonInit('POST'));
}

export function patchBranch(
  id: string,
  payload: { label?: string; note?: string; verdict?: unknown; is_baseline?: boolean }
) {
  return orchestrator<Branch>(`/branches/${id}`, jsonInit('PATCH', payload));
}

export function deleteBranch(id: string) {
  return orchestrator<{ deleted: boolean }>(`/branches/${id}`, { method: 'DELETE' });
}

export function forkBranch(
  id: string,
  payload: { overrides?: Record<string, unknown>; label?: string; note?: string }
) {
  return orchestrator<{ branch_id: string; control_port: number; status: string }>(
    `/branches/${id}/fork`,
    jsonInit('POST', payload)
  );
}

export function replayBranch(id: string, n: number, scenarioId: string) {
  return orchestrator<{ branch_ids: string[] }>(
    `/branches/${id}/replay`,
    jsonInit('POST', { n, scenario_id: scenarioId })
  );
}

export function compareBranches(ids: string[]) {
  return orchestrator<CompareResult>(`/branches/compare?ids=${encodeURIComponent(ids.join(','))}`);
}

export function diffBranch(id: string, against: string) {
  return orchestrator<DiffResult>(`/branches/${id}/diff?against=${encodeURIComponent(against)}`);
}

export function batchAction(
  experimentId: string,
  branchIds: string[],
  action: string,
  params?: Record<string, unknown>
) {
  return orchestrator<{ results: Record<string, unknown> }>(
    `/experiments/${experimentId}/batch`,
    jsonInit('POST', { branch_ids: branchIds, action, params })
  );
}

// ---- 分支自己的 control_port：前端直接打，不经过 orchestrator 转发 ----

function branchBase(controlPort: number): string {
  return `http://127.0.0.1:${controlPort}`;
}

export function getBranchState(controlPort: number) {
  return requestJson<BranchState>(branchBase(controlPort), '/state');
}

export function step(controlPort: number, awaitSubconscious = true) {
  return requestJson<StepResult>(
    branchBase(controlPort),
    '/control/step',
    jsonInit('POST', { await_subconscious: awaitSubconscious })
  );
}

export function stepN(
  controlPort: number,
  n: number,
  stopCondition?: 'n_cycles' | 'until_reply',
  awaitSubconscious = true
) {
  return requestJson<StepNResult>(
    branchBase(controlPort),
    '/control/step_n',
    jsonInit('POST', { n, stop_condition: stopCondition, await_subconscious: awaitSubconscious })
  );
}

export function backStep(controlPort: number) {
  return requestJson<{ ok: boolean; undo_depth: number }>(
    branchBase(controlPort),
    '/control/back_step',
    jsonInit('POST')
  );
}

export function backStepN(controlPort: number, n: number) {
  return requestJson<{ ok: boolean; stepped: number; undo_depth: number }>(
    branchBase(controlPort),
    '/control/back_step_n',
    jsonInit('POST', { n })
  );
}

export function pause(controlPort: number) {
  return requestJson<{ ok: boolean; status: string }>(branchBase(controlPort), '/control/pause', jsonInit('POST'));
}

export function resume(controlPort: number, maxCycles?: number, maxSeconds?: number) {
  return requestJson<{ ok: boolean; status: string }>(
    branchBase(controlPort),
    '/control/resume',
    jsonInit('POST', { max_cycles: maxCycles, max_seconds: maxSeconds })
  );
}

export function pushInput(controlPort: number, content: string, participantId = 'explore_lab') {
  return requestJson<{ event_id: string }>(
    branchBase(controlPort),
    '/input',
    jsonInit('POST', { content, participant_id: participantId })
  );
}

export function setSystemPromptOverride(controlPort: number, text: string | null) {
  return requestJson<{ active: boolean }>(
    branchBase(controlPort),
    '/system_prompt_override',
    jsonInit('PATCH', { text })
  );
}

export function getSystemPrompt(controlPort: number) {
  return requestJson<SystemPromptSnapshot>(branchBase(controlPort), '/system_prompt');
}

export function patchConfig(controlPort: number, payload: Record<string, unknown>) {
  return requestJson<{ applied: Record<string, unknown> }>(
    branchBase(controlPort),
    '/config',
    jsonInit('PATCH', payload)
  );
}

export function triggerSubconscious(controlPort: number, mode: string) {
  return requestJson<{ spawned_bubble_ids: string[] }>(
    branchBase(controlPort),
    '/subconscious/trigger',
    jsonInit('POST', { mode })
  );
}

export function setSubconsciousEnabled(controlPort: number, enabled: boolean) {
  return requestJson<{ enabled: boolean; pending: string[] }>(
    branchBase(controlPort),
    '/subconscious/enabled',
    jsonInit('PATCH', { enabled })
  );
}

export function getEventsStreamUrl(controlPort: number): string {
  return `${branchBase(controlPort)}/events`;
}

