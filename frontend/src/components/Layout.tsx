import { exportLemon, runTest } from '../api';

export function MenuBar({ onTestDone }: { onTestDone: () => void }) {
  return (
    <div className="menubar">
      <button onClick={async () => {
        const r = await exportLemon();
        alert(r.ok ? `已导出到 ${r.path}` : r.error);
      }}>导出 Lemon</button>
      <button onClick={async () => {
        const r = await runTest();
        const text = r.reports?.map((rep: any) =>
          `题目 ${rep.problem_id}:\n${rep.log.map((l: string) => `  ${l}`).join('\n')}\n` +
          (rep.errors.length ? `  错误: ${rep.errors.length}个\n` : '') +
          (rep.warnings.length ? `  警告: ${rep.warnings.length}个\n` : '')
        ).join('\n') || '测试完成';
        alert(text);
        onTestDone();
      }}>集成测试</button>
      <span style={{ marginLeft: 'auto', fontSize: 12, color: 'var(--fg-muted)' }}>OI 组题助手</span>
    </div>
  );
}

export function StatusBar({ path, usage }: { path: string; usage: any }) {
  return (
    <div className="statusbar">
      <span>{path}</span>
      {usage && (
        <span>
          📊 输入 {usage.prompt_tokens} / 输出 {usage.completion_tokens}
          {usage.cache_hit_tokens != null && ` / 缓存命中 ${usage.cache_hit_tokens}`}
        </span>
      )}
    </div>
  );
}
