import { AlertCircle } from 'lucide-react';

export function EmptyState({ title, description }: { title: string; description?: string }) {
  return (
    <div className="empty-state">
      <AlertCircle size={18} />
      <div>
        <div className="empty-title">{title}</div>
        {description && <div className="empty-description">{description}</div>}
      </div>
    </div>
  );
}
