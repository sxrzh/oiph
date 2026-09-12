import { useState } from 'react';
import type { SessionInfo } from '../types';
import { exportSession, newSession, switchSession } from '../api';
import { swAlert } from './sw';

export function SessionBar({
  sessions,
  current,
  onSwitched,
  onMessagesLoaded,
}: {
  sessions: SessionInfo[];
  current: string | null;
  onSwitched: () => void;
  onMessagesLoaded: (messages: any[], children: any[], usage?: any) => void;
}) {
  const [creating, setCreating] = useState(false);
  const [name, setName] = useState('');
  const [exportFormat, setExportFormat] = useState<'json' | 'markdown'>('json');

  const handleExport = async () => {
    if (!current) {
      swAlert('导出失败', '没有当前会话');
      return;
    }
    const r = await exportSession(current, exportFormat);
    if (r.error || !r.content) {
      swAlert('导出失败', r.error ?? '未知错误');
      return;
    }
    const ext = exportFormat === 'markdown' ? 'md' : 'json';
    const mime = exportFormat === 'markdown' ? 'text/markdown' : 'application/json';
    const blob = new Blob([r.content], { type: `${mime};charset=utf-8` });
    const url = URL.createObjectURL(blob);
    const a = document.createElement('a');
    a.href = url;
    a.download = `${r.name ?? current}.${ext}`;
    a.click();
    URL.revokeObjectURL(url);
  };

  const handleNew = async () => {
    const r = await newSession(name || undefined);
    if (r.ok) {
      setName('');
      setCreating(false);
      onSwitched();
    } else {
      swAlert('错误', r.error);
    }
  };

  const handleSwitch = async (n: string) => {
    const r = await switchSession(n);
    if (r.ok) {
      // 直接渲染返回的历史消息
      if (onMessagesLoaded) onMessagesLoaded(r.messages ?? [], r.children ?? [], r.usage);
      onSwitched();
    } else {
      swAlert('错误', r.error);
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
      <select
        value={exportFormat}
        onChange={e => setExportFormat(e.target.value as 'json' | 'markdown')}
        title="导出格式"
      >
        <option value="json">JSON</option>
        <option value="markdown">Markdown</option>
      </select>
      <button onClick={handleExport}>导出</button>
    </div>
  );
}
