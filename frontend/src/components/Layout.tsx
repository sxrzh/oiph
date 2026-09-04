import { useState } from 'react';
import { exportLemon, runTest } from '../api';
import { swAlert } from './sw';

export function MenuBar({ onTestDone }: { onTestDone: () => void }) {
  const [testing, setTesting] = useState(false);
  const [exporting, setExporting] = useState(false);
  return (
    <div className="menubar">
      <button disabled={exporting} onClick={async () => {
        setExporting(true);
        try {
          const r = await exportLemon();
          r.ok ? swAlert('导出成功', `已导出到 ${r.path}`) : swAlert('导出失败', r.error);
        } finally {
          setExporting(false);
        }
      }}>{exporting ? '导出中…' : '导出 Lemon'}</button>
      <button disabled={testing} onClick={async () => {
        setTesting(true);
        try {
          const r = await runTest();
          const text = r.reports?.map((rep: any) =>
            `题目 ${rep.problem_id}:\n${rep.log.map((l: string) => `  ${l}`).join('\n')}\n` +
            (rep.errors.length ? `  错误: ${rep.errors.length}个\n` : '') +
            (rep.warnings.length ? `  警告: ${rep.warnings.length}个\n` : '')
          ).join('\n') || '测试完成';
          swAlert('集成测试结果', text);
          onTestDone();
        } finally {
          setTesting(false);
        }
      }}>{testing ? '测试中…' : '集成测试'}</button>
      <span style={{ marginLeft: 'auto', fontSize: 12, color: 'var(--fg-muted)' }}>OI 组题助手</span>
    </div>
  );
}

export function StatusBar({
  path,
  usage,
}: {
  path: string;
  usage: { input: number; output: number; hit: number };
}) {
  const input = usage.input;
  let text = `输入 ${input}`;
  if (usage.hit > 0) {
    const pct = (usage.hit / Math.max(input, 1)) * 100;
    text += `(缓存命中 ${pct.toFixed(1)}%)`;
  }
  text += ` / 输出 ${usage.output}`;
  return (
    <div className="statusbar">
      <span>{path}</span>
      <span>📊 {text}</span>
    </div>
  );
}
