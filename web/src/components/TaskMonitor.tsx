import { CheckCircle2, CircleDot, ListTodo } from 'lucide-react';
import type { FullStatus, TaskItem } from '../api/types';
import { EmptyState } from './EmptyState';

function formatTaskTime(value?: string) {
  if (!value) return '';
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return value;
  return date.toLocaleString(undefined, {
    month: '2-digit',
    day: '2-digit',
    hour: '2-digit',
    minute: '2-digit',
  });
}

function TaskRow({ task }: { task: TaskItem }) {
  const createdAt = formatTaskTime(task.created_at);
  const updatedAt = formatTaskTime(task.updated_at);
  return (
    <div className="task-row">
      <div className={`task-status task-${task.status || 'unknown'}`} />
      <div className="task-content">
        <div className="task-title">{task.description || '未命名任务'}</div>
        <div className="task-meta">
          <span>{task.status || 'unknown'}</span>
          {task.priority && <span>{task.priority}</span>}
          {task.source && <span>from {task.source}</span>}
          {task.progress && <span>{task.progress}</span>}
          {createdAt && <span>created {createdAt}</span>}
          {updatedAt && <span>updated {updatedAt}</span>}
        </div>
      </div>
    </div>
  );
}

export function TaskMonitor({ data }: { data: FullStatus | null }) {
  const stats = data?.task_stats;
  const tasks = data?.tasks || [];
  return (
    <section className="panel-card task-monitor">
      <div className="section-header">
        <div>
          <span className="eyebrow">Tasks</span>
          <h2>最近任务观测</h2>
        </div>
        <ListTodo size={18} />
      </div>

      <div className="task-stats">
        <div><CircleDot size={14} /> 活跃 <strong>{stats?.active ?? 0}</strong></div>
        <div><ListTodo size={14} /> 待办 <strong>{stats?.pending ?? 0}</strong></div>
        <div><CheckCircle2 size={14} /> 完成 <strong>{stats?.completed ?? 0}</strong></div>
      </div>

      <div className="task-list">
        {tasks.length > 0 ? tasks.map((task, index) => <TaskRow key={task.id || index} task={task} />) : (
          <EmptyState title="暂无任务观测" description="当前状态接口没有返回任务列表。" />
        )}
      </div>
    </section>
  );
}
