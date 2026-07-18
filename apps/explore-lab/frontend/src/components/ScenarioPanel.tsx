import { useEffect, useState } from 'react';
import * as api from '../api/client';
import type { Scenario } from '../api/types';

interface Props {
  experimentId: string;
  selectedBranchId: string | null;
  onReplayed: (branchIds: string[]) => void;
}

export function ScenarioPanel({ experimentId, selectedBranchId, onReplayed }: Props) {
  const [scenarios, setScenarios] = useState<Scenario[]>([]);
  const [name, setName] = useState('');
  const [eventsText, setEventsText] = useState('');
  const [selectedScenarioId, setSelectedScenarioId] = useState('');
  const [replayN, setReplayN] = useState(3);
  const [error, setError] = useState<string | null>(null);

  const reload = () => {
    api
      .listScenarios(experimentId)
      .then(r => setScenarios(r.scenarios))
      .catch(e => setError(e instanceof Error ? e.message : '加载 Scenario 失败'));
  };

  useEffect(reload, [experimentId]);

  const createScenario = async (e: React.FormEvent) => {
    e.preventDefault();
    const events = eventsText
      .split('\n')
      .map(line => line.trim())
      .filter(Boolean)
      .map(content => ({ content }));
    if (!name.trim() || events.length === 0) return;
    try {
      await api.createScenario(experimentId, name, events);
      setName('');
      setEventsText('');
      reload();
    } catch (err) {
      setError(err instanceof Error ? err.message : '创建失败');
    }
  };

  const replay = async () => {
    if (!selectedBranchId || !selectedScenarioId) return;
    try {
      const result = await api.replayBranch(selectedBranchId, replayN, selectedScenarioId);
      onReplayed(result.branch_ids);
    } catch (err) {
      setError(err instanceof Error ? err.message : 'replay 失败');
    }
  };

  return (
    <div className="scenario-panel">
      <h3>Scenario</h3>
      {error && <p className="error-text">{error}</p>}
      <form onSubmit={createScenario} className="scenario-form">
        <input placeholder="scenario 名字" value={name} onChange={e => setName(e.target.value)} />
        <textarea
          placeholder="每行一条输入消息"
          value={eventsText}
          onChange={e => setEventsText(e.target.value)}
          rows={3}
        />
        <button type="submit">保存 Scenario</button>
      </form>

      <ul className="scenario-list">
        {scenarios.map(sc => (
          <li key={sc.id}>
            <label>
              <input
                type="radio"
                name="scenario"
                checked={selectedScenarioId === sc.id}
                onChange={() => setSelectedScenarioId(sc.id)}
              />
              {sc.name}（{sc.events.length} 条）
            </label>
          </li>
        ))}
      </ul>

      <div className="row-buttons">
        <span>replay n=</span>
        <input
          type="number"
          min={1}
          value={replayN}
          onChange={e => setReplayN(Number(e.target.value))}
          style={{ width: 48 }}
        />
        <button disabled={!selectedBranchId || !selectedScenarioId} onClick={replay}>
          对选中分支 replay
        </button>
      </div>
    </div>
  );
}
