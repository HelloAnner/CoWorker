import { BadgeCheck, CalendarDays, Fingerprint, Users } from 'lucide-react';
import type { IdentityInfo } from '../api/types';

export function IdentityCard({ identity }: { identity?: IdentityInfo }) {
  return (
    <section className="panel-card identity-card">
      <div className="section-header">
        <div>
          <span className="eyebrow">Identity</span>
          <h2>身份档案</h2>
        </div>
        <Fingerprint size={18} />
      </div>

      <div className="identity-license">
        <div className="identity-mark">{identity?.name?.slice(0, 1) || 'C'}</div>
        <div>
          <div className="identity-name">{identity?.name || '未命名'}</div>
          <div className="identity-role">{identity?.role || '身份仍在生成中'}</div>
        </div>
      </div>

      <div className="info-list">
        <div className="info-row">
          <Users size={15} />
          <span>所属团队</span>
          <strong>{identity?.team || '—'}</strong>
        </div>
        <div className="info-row">
          <CalendarDays size={15} />
          <span>诞生日</span>
          <strong>{identity?.birth || '—'}</strong>
        </div>
        <div className="info-row">
          <BadgeCheck size={15} />
          <span>存在天数</span>
          <strong>{identity?.age_days ?? '—'}</strong>
        </div>
      </div>
    </section>
  );
}
