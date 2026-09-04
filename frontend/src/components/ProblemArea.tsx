import { useState } from 'react';
import type { ProblemDetail } from '../types';
import { StatusCell } from './Status';
import { swAlert } from './sw';
import { Spoiler } from './Spoiler';
import { MarkdownEditor } from './MarkdownEditor';
import { FileEditor } from './FileEditor';

const DETAIL_TABS = ['基本信息', '题面', '题解', '数据', '辅助程序', '解法'] as const;

export function ProblemArea({
  problem,
  onRefresh,
}: {
  problem: ProblemDetail | null;
  onRefresh: () => void;
}) {
  const [activeTab, setActiveTab] = useState<(typeof DETAIL_TABS)[number]>('基本信息');
  const [editingFile, setEditingFile] = useState<string | null>(null);

  if (!problem) {
    return <div className="detail-content"><p style={{ color: 'var(--fg-muted)' }}>比赛工程未建立，请先创建比赛。</p></div>;
  }

  return (
    <>
      <div className="problem-tabs" style={{ flexDirection: 'column', padding: '4px 8px', gap: '4px' }}>
        <div style={{ display: 'flex', alignItems: 'center', gap: '8px' }}>
          <h3 style={{ flex: 1 }}>{problem.name || problem.id}</h3>
          <button className="btn" onClick={() => { onRefresh(); }}>刷新</button>
          <button className="btn" onClick={() => fetch('/api/test', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ problem: problem.id }) }).then(r => r.json()).then(d => swAlert('集成测试结果', d.reports?.map((r: any) => r.log.join('\n')).join('\n\n') || JSON.stringify(d)))}>单题自测</button>
        </div>
      </div>
      <div className="detail-tabs">
        {DETAIL_TABS.map(tab => {
          let dot: 'gray' | 'yellow' | 'green' | 'red' = 'gray';
          if (tab === '题面') dot = problem.statement_status.color;
          else if (tab === '题解') dot = problem.tutorial_status.color;
          else if (tab === '辅助程序') {
            const all = [problem.validator_status, problem.checker_status, problem.interactive_lib_status, problem.std_status];
            dot = all.some(s => s.color === 'red') ? 'red' : all.some(s => s.color === 'yellow') ? 'yellow' : all.every(s => s.color === 'green') ? 'green' : 'gray';
          } else if (tab === '解法') {
            dot = problem.sols.some(s => s.status.color === 'red') ? 'red' : problem.sols.some(s => s.status.color === 'yellow') ? 'yellow' : (problem.sols.length > 0 && problem.sols.every(s => s.status.color === 'green')) ? 'green' : 'gray';
          }
          return (
            <div
              key={tab}
              className={`detail-tab ${activeTab === tab ? 'active' : ''}`}
              onClick={() => { setActiveTab(tab); setEditingFile(null); }}
            >
              <span className={`status-dot dot-${dot}`} />
              {tab}
            </div>
          );
        })}
      </div>
      <div className="detail-content">
        {activeTab === '基本信息' && <BasicInfo problem={problem} />}
        {activeTab === '题面' && (editingFile ? (
          <FileEditor pid={problem.id} filePath={editingFile} />
        ) : (
          <div>
            <button className="btn" onClick={() => setEditingFile('statement/zh_cn.md')}>编辑题面</button>
            <div style={{ height: '500px', marginTop: '8px' }}>
              <MarkdownEditor pid={problem.id} filePath="statement/zh_cn.md" />
            </div>
          </div>
        ))}
        {activeTab === '题解' && (
          <div style={{ height: '500px' }}>
            <MarkdownEditor pid={problem.id} filePath="tutorial/zh_cn.md" />
          </div>
        )}
        {activeTab === '数据' && <DataTab problem={problem} />}
        {activeTab === '辅助程序' && (
          <AuxiliaryTab problem={problem} onOpen={(p) => setEditingFile(p)} editingFile={editingFile} />
        )}
        {activeTab === '解法' && <SolutionsTab problem={problem} onOpen={(p) => setEditingFile(p)} editingFile={editingFile} />}
      </div>
    </>
  );
}

function BasicInfo({ problem }: { problem: ProblemDetail }) {
  return (
    <table>
      <tbody>
        <tr><th>名称</th><td>{problem.name}</td></tr>
        <tr><th>类型</th><td>{problem.problem_type}</td></tr>
        <tr><th>来源</th><td>{problem.source}</td></tr>
        <tr><th>标签</th><td>{problem.tags.join(', ')}</td></tr>
        <tr><th>时间限制</th><td>{problem.time_limit_ms} ms</td></tr>
        <tr><th>空间限制</th><td>{problem.memory_limit_mb} MB</td></tr>
        <tr><th>编译选项</th><td><code>{problem.compile_flags}</code></td></tr>
        <tr><th>查重</th><td>{problem.duplicate_check ? (problem.duplicate_check.found ? `发现疑似原题（${problem.duplicate_check.matches.join('; ')}）` : '未发现原题') : '未查重'}</td></tr>
        <tr><th>上次测试</th><td>{problem.last_tested ?? '未测试'}</td></tr>
      </tbody>
    </table>
  );
}

function DataTab({ problem }: { problem: ProblemDetail }) {
  if (problem.subtasks.length === 0) {
    return <p style={{ color: 'var(--fg-muted)' }}>无 subtask 配置</p>;
  }
  return (
    <div>
      <p style={{ marginBottom: '8px' }}>数据状态: <StatusCell status={problem.data_status} /></p>
      {problem.subtasks.map((st, i) => (
        <Spoiler key={i} title={`Subtask ${i + 1}（${st.score} 分，${st.type}）${st.sample ? ' [样例]' : ''}${st.depend.length ? ` 依赖: ${st.depend.join(', ')}` : ''}`}>
          <table>
            <thead>
              <tr><th>测试点</th><th>数据来源</th></tr>
            </thead>
            <tbody>
              {st.cases.map(c => (
                <tr key={c}>
                  <td>{c}</td>
                  <td>
                    {problem.data_gen[c] !== undefined
                      ? <span>generator <code>{problem.data_gen[c]}</code></span>
                      : <span><code>{c}.in</code>{' / '}<code>{c}.ans</code></span>}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </Spoiler>
      ))}
    </div>
  );
}

function AuxiliaryTab({ problem, onOpen, editingFile }: { problem: ProblemDetail; onOpen: (path: string) => void; editingFile: string | null }) {
  return (
    <div>
      <table>
        <thead><tr><th>文件</th><th>状态</th><th>操作</th></tr></thead>
        <tbody>
          {problem.aux_files.map(f => (
            <tr key={f.path}>
              <td>{f.name}</td>
              <td><StatusCell status={
                f.name === 'generator.cpp' ? null :
                f.name === 'validator.cpp' ? problem.validator_status :
                f.name === 'checker.cpp' ? problem.checker_status :
                f.name === 'interactive_lib.cpp' ? problem.interactive_lib_status :
                null
              } /></td>
              <td>{f.exists ? <span className="file-link" onClick={() => onOpen(f.path)}>编辑</span> : <span style={{ color: 'var(--fg-muted)' }}>不存在</span>}</td>
            </tr>
          ))}
          <tr>
            <td>std ({problem.std_file})</td>
            <td><StatusCell status={problem.std_status} /></td>
            <td><span className="file-link" onClick={() => onOpen(problem.std_file)}>编辑</span></td>
          </tr>
        </tbody>
      </table>
      {editingFile && (
        <div style={{ marginTop: '12px', height: '400px' }}>
          <FileEditor pid={problem.id} filePath={editingFile} />
        </div>
      )}
    </div>
  );
}

function SolutionsTab({ problem, onOpen, editingFile }: { problem: ProblemDetail; onOpen: (path: string) => void; editingFile: string | null }) {
  return (
    <div>
      <table>
        <thead><tr><th>名称</th><th>文件</th><th>预期结果</th><th>状态</th><th>操作</th></tr></thead>
        <tbody>
          <tr>
            <td>std</td>
            <td>{problem.std_file}</td>
            <td>AC</td>
            <td><StatusCell status={problem.std_status} /></td>
            <td><span className="file-link" onClick={() => onOpen(problem.std_file)}>编辑</span></td>
          </tr>
          {problem.sols.map(s => (
            <tr key={s.name}>
              <td>{s.name}</td>
              <td>{s.file || `solutions/${s.name}.cpp`}</td>
              <td>{s.expected_verdict} {s.expected_score ?? ''}</td>
              <td><StatusCell status={s.status} /></td>
              <td><span className="file-link" onClick={() => onOpen(s.file || `solutions/${s.name}.cpp`)}>编辑</span></td>
            </tr>
          ))}
        </tbody>
      </table>
      {editingFile && (
        <div style={{ marginTop: '12px', height: '400px' }}>
          <FileEditor pid={problem.id} filePath={editingFile} />
        </div>
      )}
    </div>
  );
}
