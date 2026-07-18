import { useCallback, useEffect, useState } from 'react';
import * as api from './api/client';
import type { Branch, ExperimentSummary } from './api/types';
import { ImportPanel } from './components/ImportPanel';
import { BranchTree } from './components/BranchTree';
import { BranchDetail } from './components/BranchDetail';
import { CompareView } from './components/CompareView';
import { ScenarioPanel } from './components/ScenarioPanel';

type Tab = 'detail' | 'compare' | 'scenarios';

export default function App() {
  const [experiments, setExperiments] = useState<ExperimentSummary[]>([]);
  const [selectedExperimentId, setSelectedExperimentId] = useState<string | null>(null);
  const [branches, setBranches] = useState<Branch[]>([]);
  const [selectedBranchId, setSelectedBranchId] = useState<string | null>(null);
  const [compareIds, setCompareIds] = useState<string[]>([]);
  const [tab, setTab] = useState<Tab>('detail');
  const [showImport, setShowImport] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const reloadExperiments = useCallback(() => {
    api
      .listExperiments()
      .then(r => {
        setExperiments(r.experiments);
        setError(null);
      })
      .catch(e => setError(e instanceof Error ? e.message : '加载实验列表失败'));
  }, []);

  useEffect(reloadExperiments, [reloadExperiments]);

  const reloadBranches = useCallback((experimentId: string) => {
    api
      .getExperiment(experimentId)
      .then(r => {
        setBranches(r.branches);
        setError(null);
      })
      .catch(e => setError(e instanceof Error ? e.message : '加载分支失败'));
  }, []);

  useEffect(() => {
    if (selectedExperimentId) reloadBranches(selectedExperimentId);
    else setBranches([]);
  }, [reloadBranches, selectedExperimentId]);

  useEffect(() => {
    if (!selectedExperimentId) return;
    const refreshCurrentExperiment = () => {
      if (document.visibilityState === 'visible') reloadBranches(selectedExperimentId);
    };
    window.addEventListener('focus', refreshCurrentExperiment);
    document.addEventListener('visibilitychange', refreshCurrentExperiment);
    return () => {
      window.removeEventListener('focus', refreshCurrentExperiment);
      document.removeEventListener('visibilitychange', refreshCurrentExperiment);
    };
  }, [reloadBranches, selectedExperimentId]);

  const selectedBranch = branches.find(b => b.id === selectedBranchId) || null;

  const handleImported = (experimentId: string) => {
    setShowImport(false);
    reloadExperiments();
    setSelectedExperimentId(experimentId);
  };

  const handleForked = (newBranchId: string) => {
    if (selectedExperimentId) reloadBranches(selectedExperimentId);
    setSelectedBranchId(newBranchId);
    setTab('detail');
  };

  const toggleCompare = (id: string) => {
    setCompareIds(ids => (ids.includes(id) ? ids.filter(x => x !== id) : [...ids, id]));
  };

  return (
    <div className="app-shell">
      <aside className="sidebar">
        <div className="sidebar-header">
          <h1>搭档探索实验室</h1>
          <button onClick={() => setShowImport(v => !v)}>{showImport ? '取消' : '+ 导入'}</button>
        </div>
        {showImport && <ImportPanel onImported={handleImported} />}

        <h2>实验</h2>
        <ul className="experiment-list">
          {experiments.map(exp => (
            <li key={exp.id}>
              <button
                className={selectedExperimentId === exp.id ? 'selected' : ''}
                onClick={() => setSelectedExperimentId(exp.id)}
              >
                {exp.id}（{exp.branch_count} 分支）
              </button>
            </li>
          ))}
        </ul>

        {selectedExperimentId && (
          <>
            <h2>分支树</h2>
            <BranchTree
              branches={branches}
              selectedId={selectedBranchId}
              compareIds={compareIds}
              onSelect={id => {
                setSelectedBranchId(id);
                setTab('detail');
              }}
              onToggleCompare={toggleCompare}
            />
            <div className="row-buttons">
              <button disabled={compareIds.length === 0} onClick={() => setTab('compare')}>
                对比选中（{compareIds.length}）
              </button>
              <button onClick={() => setTab('scenarios')}>Scenario</button>
            </div>
          </>
        )}
      </aside>

      <main className="main-panel">
        {error && <p className="error-text">{error}</p>}
        {tab === 'detail' && selectedBranch && (
          <BranchDetail
            branch={selectedBranch}
            onForked={handleForked}
            onBranchPatched={() => selectedExperimentId && reloadBranches(selectedExperimentId)}
          />
        )}
        {tab === 'detail' && !selectedBranch && <p className="empty-hint">从左侧选一个分支。</p>}
        {tab === 'compare' && <CompareView branchIds={compareIds} />}
        {tab === 'scenarios' && selectedExperimentId && (
          <ScenarioPanel
            experimentId={selectedExperimentId}
            selectedBranchId={selectedBranchId}
            onReplayed={ids => {
              setCompareIds(ids);
              setTab('compare');
              if (selectedExperimentId) reloadBranches(selectedExperimentId);
            }}
          />
        )}
      </main>
    </div>
  );
}
