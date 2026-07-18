import { Brain, Cpu, Database, Orbit } from 'lucide-react';
import type { BasicStatus, FullStatus } from '../api/types';

function Metric({ icon, label, value }: { icon: React.ReactNode; label: string; value: React.ReactNode }) {
  return (
    <div className="metric-card">
      <div className="metric-icon">{icon}</div>
      <span>{label}</span>
      <strong>{value}</strong>
    </div>
  );
}

export function VitalsGrid({ data, basicStatus }: { data: FullStatus | null; basicStatus: BasicStatus | null }) {
  const vitals = data?.vitals;
  return (
    <section className="panel-card">
      <div className="section-header">
        <div>
          <span className="eyebrow">Vitals</span>
          <h2>生命体征</h2>
        </div>
        <Orbit size={18} />
      </div>

      <div className="metric-grid">
        <Metric icon={<Cpu size={17} />} label="循环次数" value={vitals?.cycle_count ?? basicStatus?.cycle_count ?? '—'} />
        <Metric icon={<Brain size={17} />} label="技能数量" value={vitals?.skill_count ?? '—'} />
        <Metric icon={<Database size={17} />} label="记忆数量" value={vitals?.memory_count ?? '—'} />
        <Metric icon={<Orbit size={17} />} label="版本" value={data?.version || '—'} />
      </div>
    </section>
  );
}
