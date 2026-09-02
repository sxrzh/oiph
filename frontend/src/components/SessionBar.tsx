import { useState } from 'react';
import type { SessionInfo } from '../types';
import { newSession, switchSession } from '../api';

export function SessionBar({
  sessions,
  current,
  onSwitched,
}: {
  sessions: SessionInfo[];
  current: string | null;
  onSwitched: () => void;
}) {
  const [creating, setCreating] = useState(false);
  const [name, setName] = useState('');

  const handleNew = async () => {
    const r = await newSession(name || undefined);
    if (r.ok) {
      setName('');
      setCreating(false);
      onSwitched();
    } else {
      alert(r.error);
    }
  };

  const handleSwitch = async (n: string) => {
    const r = await switchSession(n);
    if (r.ok) {
      onSwitched();
    } else {
      alert(r.error);
    }
  };

  return (
    <div className="session-bar">
      <select value={current ?? ''} onChange={e => handleSwitch(e.target.value)}>
        {sessions.length === 0 && <option value="">（无会话）</option>}
        {sessions.map(s => (
          <option key={s.name} value={s.name}>
            {s.name} ({s.messages}条)
          </option>
        ))}
      </select>
      {creating ? (
        <>
          <input
            style={{ background: 'var(--bg-panel)', color: 'var(--fg)', border: '1px solid var(--border)', borderRadius: 4, padding: '2px 6px', fontSize: 12, width: 120 }}
            placeholder="会话名"
            value={name}
            onChange={e => setName(e.target.value)}
            onKeyDown={e => { if (e.key === 'Enter') handleNew(); }}
          />
          <button onClick={handleNew}>确定</button>
          <button onClick={() => setCreating(false)}>取消</button>
        </>
      ) : (
        <button onClick={() => setCreating(true)}>新建</button>
      )}
    </div>
  );
}
