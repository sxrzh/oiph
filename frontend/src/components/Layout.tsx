import { useState } from 'react';
import type { BudgetInfo } from '../api';
import { exportLemon, runTest } from '../api';
import { swAlert } from './sw';

export function MenuBar({ onTestDone, budget }: { onTestDone: () => void; budget?: BudgetInfo | null }) {
  const overWarn = budget ? budget.limit - budget.used < budget.warn : false;
  const [testing, setTesting] = useState(false);
  const [exporting, setExporting] = useState(false);
  return (
    <div className="menubar">
      <button disabled={exporting} onClick={async () => {
        setExporting(true);
        try {
          const r = await exportLemon();
          if (r.ok) {
            const warns: string[] = r.warnings ?? [];
            swAlert(
              '导出成功',
              `已导出到 ${r.path}` + (warns.length ? `\n\n警告：\n${warns.join('\n')}` : ''),
            );
          } else {
            swAlert('导出失败', r.error ?? '未知错误');
          }
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
      <button onClick={() => window.open('./settings.html', '_blank')}>设置</button>
      {overWarn && budget && (
        <span className="budget-warning" style={{ position: 'absolute', left: '50%', transform: 'translateX(-50%)' }}>
          ⚠ 预算即将耗尽：已用 {budget.used.toFixed(2)} / {budget.limit} {budget.currency}
        </span>
      )}
      <span style={{ marginLeft: 'auto', fontSize: 12, color: 'var(--fg-muted)' }}>OI 组题助手</span>
    </div>
  );
}

export function StatusBar({
  path,
  usage,
}: {
  path: string;
  usage: { input: number; output: number; hit: number; cost?: { currency: string; amount: number } | null; budget?: BudgetInfo | null };
}) {
  const input = usage.input;
  let text = `输入 ${input}`;
  if (usage.hit > 0) {
    const pct = (usage.hit / Math.max(input, 1)) * 100;
    text += `(缓存命中 ${pct.toFixed(1)}%)`;
  }
  text += ` / 输出 ${usage.output}`;
  if (usage.cost && usage.cost.amount > 0) {
    text += ` / 花费 ${usage.cost.amount.toFixed(4)} ${usage.cost.currency}`;
  }
  if (usage.budget) {
    text += `（预算 ${usage.budget.used.toFixed(2)}/${usage.budget.limit} ${usage.budget.currency}）`;
  }
  return (
    <div className="statusbar">
      <span>{path}</span>
      <span>📊 {text}</span>
    </div>
  );
}
