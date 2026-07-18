import { GitBranch, Sparkle } from 'lucide-react';
import type { BubbleItem, FullStatus } from '../api/types';
import { EmptyState } from './EmptyState';

function BubbleRow({ bubble }: { bubble: BubbleItem }) {
  return (
    <div className="bubble-row">
      <div className="bubble-icon"><Sparkle size={14} /></div>
      <div>
        <div className="bubble-goal">{bubble.goal || '后台思考'}</div>
        <div className="task-meta">
          <span>{bubble.status || 'unknown'}</span>
          <span>{bubble.cycles ?? 0} cycles</span>
        </div>
      </div>
    </div>
  );
}

export function BubbleList({ data }: { data: FullStatus | null }) {
  const bubbles = data?.bubbles || [];
  return (
    <section className="panel-card">
      <div className="section-header">
        <div>
          <span className="eyebrow">Bubbles</span>
          <h2>后台思考</h2>
        </div>
        <GitBranch size={18} />
      </div>
      <div className="bubble-list">
        {bubbles.length > 0 ? bubbles.map((bubble, index) => (
          <BubbleRow key={bubble.id || index} bubble={bubble} />
        )) : (
          <EmptyState title="暂无泡泡任务" description="后台子任务出现时会在这里浮现。" />
        )}
      </div>
    </section>
  );
}
