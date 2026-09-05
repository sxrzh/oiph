import { useEffect, useState } from 'react';
import { CodeEditor } from './components/CodeEditor';
import { swAlert } from './components/sw';

// ---------------------------------------------------------------------------
// 类型
// ---------------------------------------------------------------------------

interface Pricing {
  mode: 'auto' | 'manual';
  input: number | null;
  hit: number | null;
  output: number | null;
  currency?: string;
}

interface AgentConf {
  name: string;
  base_url: string | null;
  api_key: string | null;
  reasoning: boolean | null;
  max_context: number | null;
  prompt: string | null;
  pricing: Pricing;
}

interface KbFile {
  scope: 'global' | 'project';
  name: string;
  chunks: number;
  has_source: boolean;
}

interface SkillItem {
  name: string;
  description: string;
  scope: 'global' | 'project';
}

// ---------------------------------------------------------------------------
// 工具
// ---------------------------------------------------------------------------

async function jfetch(url: string, opts?: RequestInit): Promise<any> {
  const r = await fetch(url, opts);
  return r.json();
}

const post = (url: string, body: any) =>
  jfetch(url, { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify(body) });

// ---------------------------------------------------------------------------
// 第一栏：API 配置
// ---------------------------------------------------------------------------

function BudgetCard() {
  const [limit, setLimit] = useState<number | ''>('');
  const [warn, setWarn] = useState<number | ''>('');
  const [currency, setCurrency] = useState('CNY');
  const [used, setUsed] = useState<number | null>(null);
  const [enabled, setEnabled] = useState(false);
  const [busy, setBusy] = useState(false);

  const load = async () => {
    const d = await jfetch('/api/settings/budget');
    if (d.budget) {
      setEnabled(true);
      setLimit(d.budget.limit);
      setWarn(d.budget.warn);
      setCurrency(d.budget.currency);
      setUsed(d.budget.used);
    } else {
      setEnabled(false);
      setLimit(100);
      setWarn(10);
      setUsed(0);
    }
  };
  useEffect(() => { load(); }, []);

  const saveBudget = async () => {
    if (limit === '' || warn === '') { swAlert('填写不完整', '请填写 limit 和 warn'); return; }
    setBusy(true);
    try {
      const d = await post('/api/settings/budget', { limit, warn, currency });
      if (d.error) { swAlert('保存失败', d.error); return; }
      swAlert('已保存', `费用预算已更新：${d.budget.used.toFixed(2)} / ${d.budget.limit} ${d.budget.currency}`);
      load();
    } finally {
      setBusy(false);
    }
  };

  const resetUsed = async () => {
    if (!window.confirm('确定重置预算已用量（used = 0）？')) return;
    setBusy(true);
    try {
      const d = await post('/api/fee/reset', {});
      if (d.error) { swAlert('重置失败', d.error); return; }
      swAlert('已重置', `已用量清零（limit ${d.budget.limit} ${d.budget.currency} 不变）`);
      load();
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="settings-card">
      <h3>费用预算</h3>
      {!enabled && <p className="hint">尚未配置预算（保存后启用，费用将随使用累加）</p>}
      <div className="row">
        <div>
          <label>预算上限（limit）</label>
          <input type="number" step="0.01" value={limit}
            onChange={e => setLimit(e.target.value === '' ? '' : Number(e.target.value))} />
        </div>
        <div>
          <label>告警阈值（warn）</label>
          <input type="number" step="0.01" value={warn}
            onChange={e => setWarn(e.target.value === '' ? '' : Number(e.target.value))} />
        </div>
        <div>
          <label>货币</label>
          <select value={currency} onChange={e => setCurrency(e.target.value)}>
            <option value="CNY">CNY</option>
            <option value="USD">USD</option>
            <option value="EUR">EUR</option>
            <option value="JPY">JPY</option>
            <option value="GBP">GBP</option>
          </select>
        </div>
      </div>
      <p className="hint">已用：{used === null ? '-' : used.toFixed(4)} {currency}（计价货币不同时自动按 frankfurter.dev 汇率换算）</p>
      <div className="row">
        <button className="btn" disabled={busy} onClick={saveBudget}>{busy ? '处理中…' : '保存预算'}</button>
        <button className="btn" disabled={busy || !enabled} onClick={resetUsed}>重置已用量</button>
      </div>
    </div>
  );
}

function ApiConfigPane() {
  const [agents, setAgents] = useState<AgentConf[]>([]);
  const [saving, setSaving] = useState<string | null>(null);

  const load = async () => {
    const d = await jfetch('/api/settings/agents');
    if (d.error) { swAlert('加载失败', d.error); return; }
    setAgents(d.agents);
  };
  useEffect(() => { load(); }, []);

  const patch = (name: string, fields: Partial<AgentConf>) =>
    setAgents(a => a.map(x => (x.name === name ? { ...x, ...fields } : x)));

  const save = async (ag: AgentConf) => {
    setSaving(ag.name);
    try {
      const d = await post('/api/settings/agents', {
        agents: {
          [ag.name]: {
            base_url: ag.base_url || null,
            api_key: ag.api_key || null,
            reasoning: ag.reasoning === null || ag.reasoning === undefined
              ? 'default'
              : ag.reasoning ? 'on' : 'off',
            max_context: ag.max_context,
            pricing_mode: ag.pricing.mode,
            price: ag.pricing.mode === 'manual'
              ? { input: ag.pricing.input ?? 0, hit: ag.pricing.hit ?? 0, output: ag.pricing.output ?? 0, currency: ag.pricing.currency ?? 'CNY' }
              : null,
          },
        },
      });
      if (d.error) swAlert('保存失败', d.error);
      else swAlert('已保存', `agent '${ag.name}' 的 API 配置已保存并实时生效（用量统计继续累计）`);
    } finally {
      setSaving(null);
    }
  };

  return (
    <div className="settings-pane">
      <h2>API 配置</h2>
      <BudgetCard />
      <p className="hint">留空 Base URL / API Key 时回退全局命令行参数。保存后立即生效，无需重启。</p>
      {agents.map(ag => (
        <div key={ag.name} className="settings-card">
          <h3>{ag.name}</h3>
          <label>Base URL</label>
          <input value={ag.base_url ?? ''} placeholder="使用全局"
            onChange={e => patch(ag.name, { base_url: e.target.value })} />
          <label>API Key</label>
          <input type="password" value={ag.api_key ?? ''} placeholder="使用全局"
            onChange={e => patch(ag.name, { api_key: e.target.value })} />
          <div className="row">
            <div>
              <label>思考模式</label>
              <select value={ag.reasoning === null || ag.reasoning === undefined ? 'default' : ag.reasoning ? 'on' : 'off'}
                onChange={e => patch(ag.name, { reasoning: e.target.value === 'default' ? null : e.target.value === 'on' })}>
                <option value="default">默认（不发送）</option>
                <option value="on">开启</option>
                <option value="off">关闭</option>
              </select>
            </div>
            <div>
              <label>上下文长度</label>
              <input type="number" value={ag.max_context ?? 1048576}
                onChange={e => patch(ag.name, { max_context: Number(e.target.value) || null })} />
            </div>
          </div>
          <label>计价</label>
          <select value={ag.pricing.mode}
            onChange={e => patch(ag.name, {
              pricing: {
                mode: e.target.value as 'auto' | 'manual',
                input: ag.pricing.input, hit: ag.pricing.hit, output: ag.pricing.output,
                currency: ag.pricing.currency,
              },
            })}>
            <option value="auto">自动识别（按 base_url 判断供应商）</option>
            <option value="manual">输入</option>
          </select>
          {ag.pricing.mode === 'manual' && (
            <div className="row prices">
              <div>
                <label>输入（未命中）/M</label>
                <input type="number" step="0.01" value={ag.pricing.input ?? ''}
                  onChange={e => patch(ag.name, { pricing: { ...ag.pricing, input: Number(e.target.value) } })} />
              </div>
              <div>
                <label>输入（命中）/M</label>
                <input type="number" step="0.01" value={ag.pricing.hit ?? ''}
                  onChange={e => patch(ag.name, { pricing: { ...ag.pricing, hit: Number(e.target.value) } })} />
              </div>
              <div>
                <label>输出 /M</label>
                <input type="number" step="0.01" value={ag.pricing.output ?? ''}
                  onChange={e => patch(ag.name, { pricing: { ...ag.pricing, output: Number(e.target.value) } })} />
              </div>
              <div>
                <label>货币</label>
                <select value={ag.pricing.currency ?? 'CNY'}
                  onChange={e => patch(ag.name, { pricing: { ...ag.pricing, currency: e.target.value } })}>
                  <option value="CNY">CNY</option>
                  <option value="USD">USD</option>
                  <option value="EUR">EUR</option>
                  <option value="JPY">JPY</option>
                </select>
              </div>
            </div>
          )}
          <button className="btn" disabled={saving === ag.name} onClick={() => save(ag)}>
            {saving === ag.name ? '保存中…' : '保存'}
          </button>
        </div>
      ))}
    </div>
  );
}

// ---------------------------------------------------------------------------
// 第二栏：知识库管理
// ---------------------------------------------------------------------------

function KbPane() {
  const [files, setFiles] = useState<KbFile[]>([]);
  const [selected, setSelected] = useState<KbFile | null>(null);
  const [content, setContent] = useState('');

  const load = async () => {
    const d = await jfetch('/api/kb/files');
    setFiles(d.files ?? []);
  };
  useEffect(() => { load(); }, []);

  const view = async (f: KbFile) => {
    const d = await jfetch(`/api/kb/file?scope=${f.scope}&name=${encodeURIComponent(f.name)}`);
    if (d.error) { swAlert('读取失败', d.error); return; }
    setSelected(f);
    setContent(d.content);
  };

  const addFile = async (scope: 'global' | 'project') => {
    const inp = document.createElement('input');
    inp.type = 'file';
    inp.accept = '.md,.txt';
    inp.onchange = async () => {
      const file = inp.files?.[0];
      if (!file) return;
      const content = await file.text();
      const d = await post('/api/kb/add', { scope, name: file.name, content });
      if (d.error) { swAlert('添加失败', d.error); return; }
      swAlert('已添加', `${file.name} 已加入${scope === 'global' ? '全局' : '工程'}知识库（${d.chunks} 个分块）`);
      load();
    };
    inp.click();
  };

  const del = async (f: KbFile) => {
    if (!window.confirm(`确定从${f.scope === 'global' ? '全局' : '工程'}知识库删除 '${f.name}'？`)) return;
    const d = await post('/api/kb/delete', { scope: f.scope, name: f.name });
    if (d.error) { swAlert('删除失败', d.error); return; }
    if (selected?.name === f.name && selected.scope === f.scope) setSelected(null);
    load();
  };

  return (
    <div className="settings-pane">
      <h2>知识库管理</h2>
      <div className="row">
        <button className="btn" onClick={() => addFile('global')}>添加到全局</button>
        <button className="btn" onClick={() => addFile('project')}>添加到工程</button>
        <button className="btn" onClick={load}>刷新</button>
      </div>
      <div className="file-list">
        {files.map(f => (
          <div key={`${f.scope}/${f.name}`} className={`file-item ${selected === f ? 'active' : ''}`}>
            <span className="file-name" onClick={() => view(f)}>
              [{f.scope === 'global' ? '全局' : '工程'}] {f.name} <small>({f.chunks} 分块)</small>
            </span>
            <button className="btn btn-danger" onClick={() => del(f)}>删除</button>
          </div>
        ))}
        {files.length === 0 && <p className="hint">知识库为空</p>}
      </div>
      {selected && (
        <div className="viewer">
          <h3>查看：{selected.name}（只读）</h3>
          <div className="code-viewer">
            <CodeEditor value={content} onChange={() => {}} language="markdown" readOnly />
          </div>
        </div>
      )}
    </div>
  );
}

// ---------------------------------------------------------------------------
// 第三栏：Skills 管理
// ---------------------------------------------------------------------------

function SkillsPane() {
  const [skills, setSkills] = useState<SkillItem[]>([]);
  const [selected, setSelected] = useState<string | null>(null);
  const [content, setContent] = useState('');

  const load = async () => {
    const d = await jfetch('/api/skills');
    setSkills(d.skills ?? []);
  };
  useEffect(() => { load(); }, []);

  const view = async (name: string) => {
    const d = await jfetch(`/api/skill/file?name=${encodeURIComponent(name)}`);
    if (d.error) { swAlert('读取失败', d.error); return; }
    setSelected(name);
    setContent(d.content);
  };

  const addSkill = async (scope: 'global' | 'project') => {
    const name = window.prompt('Skill 名称（目录名，英文/数字/连字符）：');
    if (!name) return;
    const inp = document.createElement('input');
    inp.type = 'file';
    inp.accept = '.md';
    inp.onchange = async () => {
      const file = inp.files?.[0];
      if (!file) return;
      const content = await file.text();
      const d = await post('/api/skill/add', { scope, name, content });
      if (d.error) { swAlert('添加失败', d.error); return; }
      swAlert('已添加', `skill '${name}' 已创建（SKILL.md）`);
      load();
    };
    inp.click();
  };

  const del = async (s: SkillItem) => {
    if (!window.confirm(`确定删除 skill '${s.name}'（整个目录）？`)) return;
    const d = await post('/api/skill/delete', { scope: s.scope, name: s.name });
    if (d.error) { swAlert('删除失败', d.error); return; }
    if (selected === s.name) setSelected(null);
    load();
  };

  return (
    <div className="settings-pane">
      <h2>Skills 管理</h2>
      <div className="row">
        <button className="btn" onClick={() => addSkill('global')}>添加到全局</button>
        <button className="btn" onClick={() => addSkill('project')}>添加到工程</button>
        <button className="btn" onClick={load}>刷新</button>
      </div>
      <div className="file-list">
        {skills.map(s => (
          <div key={s.name} className={`file-item ${selected === s.name ? 'active' : ''}`}>
            <span className="file-name" onClick={() => view(s.name)} title={s.description}>
              [{s.scope === 'global' ? '全局' : '工程'}] {s.name} <small>— {s.description.slice(0, 40)}</small>
            </span>
            <button className="btn btn-danger" onClick={() => del(s)}>删除</button>
          </div>
        ))}
        {skills.length === 0 && <p className="hint">没有 skills</p>}
      </div>
      {selected && (
        <div className="viewer">
          <h3>查看：{selected}（SKILL.md）</h3>
          <div className="code-viewer">
            <CodeEditor value={content} onChange={() => {}} language="markdown" readOnly />
          </div>
        </div>
      )}
    </div>
  );
}

// ---------------------------------------------------------------------------

export function SettingsPage() {
  return (
    <div className="settings-layout">
      <ApiConfigPane />
      <KbPane />
      <SkillsPane />
    </div>
  );
}
