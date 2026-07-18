import type { Branch } from '../api/types';

interface Props {
  branches: Branch[];
  selectedId: string | null;
  compareIds: string[];
  onSelect: (id: string) => void;
  onToggleCompare: (id: string) => void;
}

function statusIcon(status: string): string {
  switch (status) {
    case 'running':
      return '▶';
    case 'stepping':
      return '⏵';
    case 'paused':
      return '⏸';
    case 'crashed':
      return '✗';
    case 'stopped':
      return '■';
    default:
      return '…';
  }
}

function verdictIcon(verdict: Branch['verdict']): string {
  if (!verdict) return '';
  if (verdict.result === 'pass') return '✓';
  if (verdict.result === 'fail') return '✗';
  return '?';
}

function buildForest(branches: Branch[]): Map<string | null, Branch[]> {
  const byParent = new Map<string | null, Branch[]>();
  for (const b of branches) {
    const list = byParent.get(b.parent_id) || [];
    list.push(b);
    byParent.set(b.parent_id, list);
  }
  return byParent;
}

function BranchNode({
  branch,
  byParent,
  depth,
  selectedId,
  compareIds,
  onSelect,
  onToggleCompare,
}: {
  branch: Branch;
  byParent: Map<string | null, Branch[]>;
  depth: number;
  selectedId: string | null;
  compareIds: string[];
  onSelect: (id: string) => void;
  onToggleCompare: (id: string) => void;
}) {
  const children = byParent.get(branch.id) || [];
  return (
    <div className="branch-node" style={{ marginLeft: depth * 16 }}>
      <div className={`branch-row ${selectedId === branch.id ? 'selected' : ''}`}>
        <input
          type="checkbox"
          checked={compareIds.includes(branch.id)}
          onChange={() => onToggleCompare(branch.id)}
          title="加入对比"
        />
        <span className="branch-status" title={branch.status}>
          {statusIcon(branch.status)}
        </span>
        <button className="branch-label" onClick={() => onSelect(branch.id)}>
          {branch.is_baseline && <span className="baseline-tag">baseline</span>}
          {branch.label || branch.id}
          {verdictIcon(branch.verdict) && <span className="verdict-tag">{verdictIcon(branch.verdict)}</span>}
        </button>
      </div>
      {children.map(child => (
        <BranchNode
          key={child.id}
          branch={child}
          byParent={byParent}
          depth={depth + 1}
          selectedId={selectedId}
          compareIds={compareIds}
          onSelect={onSelect}
          onToggleCompare={onToggleCompare}
        />
      ))}
    </div>
  );
}

export function BranchTree({ branches, selectedId, compareIds, onSelect, onToggleCompare }: Props) {
  const byParent = buildForest(branches);
  const roots = byParent.get(null) || [];
  if (branches.length === 0) {
    return <p className="empty-hint">这个实验下还没有分支。</p>;
  }
  return (
    <div className="branch-tree">
      {roots.map(root => (
        <BranchNode
          key={root.id}
          branch={root}
          byParent={byParent}
          depth={0}
          selectedId={selectedId}
          compareIds={compareIds}
          onSelect={onSelect}
          onToggleCompare={onToggleCompare}
        />
      ))}
    </div>
  );
}
