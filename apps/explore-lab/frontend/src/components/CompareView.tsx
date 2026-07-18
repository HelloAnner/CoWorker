import { useEffect, useState } from 'react';
import * as api from '../api/client';
import type { CompareResult } from '../api/types';
import { TranscriptView } from './TranscriptView';

interface Props {
  branchIds: string[];
}

export function CompareView({ branchIds }: Props) {
  const [result, setResult] = useState<CompareResult | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (branchIds.length === 0) {
      setResult(null);
      return;
    }
    let active = true;
    api
      .compareBranches(branchIds)
      .then(r => active && setResult(r))
      .catch(e => active && setError(e instanceof Error ? e.message : '对比失败'));
    return () => {
      active = false;
    };
  }, [branchIds.join(',')]);

  if (branchIds.length === 0) {
    return <p className="empty-hint">在左侧分支树里勾选至少一个分支进行对比。</p>;
  }
  if (error) return <p className="error-text">{error}</p>;
  if (!result) return <p>加载中…</p>;

  return (
    <div className="compare-view">
      {branchIds.map(id => {
        const b = result.branches[id];
        if (!b) return null;
        return (
          <div className="compare-column" key={id}>
            <h3>
              {b.label || id} {b.is_baseline && <span className="baseline-tag">baseline</span>}
            </h3>
            <p className="hint">
              status={b.status}, cycle={b.cycle_count}
            </p>
            <TranscriptView messages={b.transcript} />
          </div>
        );
      })}
    </div>
  );
}
