import { useState } from 'react';
import { importExperiment } from '../api/client';

interface Props {
  onImported: (experimentId: string) => void;
}

export function ImportPanel({ onImported }: Props) {
  const [baseUrl, setBaseUrl] = useState('http://127.0.0.1:8000');
  const [token, setToken] = useState('');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const submit = async (e: React.FormEvent) => {
    e.preventDefault();
    setBusy(true);
    setError(null);
    try {
      const result = await importExperiment({ coworker_base_url: baseUrl, admin_token: token });
      setToken('');
      onImported(result.experiment_id);
    } catch (err) {
      setError(err instanceof Error ? err.message : '导入失败');
    } finally {
      setBusy(false);
    }
  };

  return (
    <form className="import-panel" onSubmit={submit}>
      <h3>导入生产配置</h3>
      <label>
        coworker_base_url
        <input
          type="text"
          value={baseUrl}
          onChange={e => setBaseUrl(e.target.value)}
          placeholder="http://127.0.0.1:8000"
          required
        />
      </label>
      <label>
        admin_token
        <input
          type="password"
          value={token}
          onChange={e => setToken(e.target.value)}
          autoComplete="off"
          required
        />
      </label>
      <button type="submit" disabled={busy}>
        {busy ? '导入中…' : '导入并创建根分支'}
      </button>
      {error && <p className="error-text">{error}</p>}
    </form>
  );
}
