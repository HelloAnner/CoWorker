import { Activity, RefreshCw, Sparkles } from 'lucide-react';
import type { BasicStatus, FullStatus } from '../api/types';

function resolveStatus(data: FullStatus | null, basic: BasicStatus | null) {
  if (data?.status === 'not_started' || basic?.status === 'not_started') return 'not_started';
  return data?.vitals?.status || (basic?.is_sleeping ? 'sleeping' : basic?.is_running ? 'running' : 'idle');
}

export function HeroPanel({
  data,
  basicStatus,
  error,
  lastFetchedAt,
  onRefresh,
}: {
  data: FullStatus | null;
  basicStatus: BasicStatus | null;
  error: string | null;
  lastFetchedAt: string | null;
  onRefresh: () => void;
}) {
  const identity = data?.identity;
  const vitals = data?.vitals;
  const status = error ? 'error' : resolveStatus(data, basicStatus);
  const model = vitals?.model || basicStatus?.model || '—';
  const provider = vitals?.provider || basicStatus?.provider || '—';

  return (
    <section className="hero-panel panel-card">
      <div className="hero-title-block">
        <div className="hero-avatar">
          <Sparkles size={22} />
        </div>
        <div>
          <div className="eyebrow">Coworker Console</div>
          <h1>{identity?.name || 'Coworker'}</h1>
          <p>{identity?.role || '一个持续运行、会记忆也会行动的 AI 协作者'}</p>
        </div>
      </div>

      <div className="hero-meta">
        <span className={`status-pill status-${status}`}>
          <Activity size={14} />
          {status}
        </span>
        <span className="model-pill">{provider} / {model}</span>
        <span className="time-pill">
          {lastFetchedAt ? new Date(lastFetchedAt).toLocaleTimeString() : '等待同步'}
        </span>
        <button className="icon-button" onClick={onRefresh} title="刷新状态">
          <RefreshCw size={16} />
        </button>
      </div>
    </section>
  );
}
