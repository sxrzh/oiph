import { useEffect, useRef, useState } from 'react';
import { getFile, saveFile } from '../api';
import { swAlert, swConfirm } from './sw';
import { CodeEditor, type EditorLanguage } from './CodeEditor';

function langForPath(path: string): EditorLanguage {
  const lower = path.toLowerCase();
  if (lower.endsWith('.md')) return 'markdown';
  return 'cpp'; // .cpp/.h/.in/.ans 等 C++ 竞赛文件默认 cpp 高亮
}

export function FileEditor({ pid, filePath }: { pid: string; filePath: string }) {
  const [content, setContent] = useState('');
  const [original, setOriginal] = useState('');
  const [dirty, setDirty] = useState(false);
  const [saved, setSaved] = useState(false);
  const [error, setError] = useState('');
  const pollRef = useRef<ReturnType<typeof setInterval> | null>(null);

  useEffect(() => {
    setDirty(false);
    setError('');
    getFile(`${pid}/${filePath}`).then(d => {
      if (d.error) {
        setError(d.error);
        setContent('');
        setOriginal('');
      } else {
        setContent(d.content);
        setOriginal(d.content);
      }
    });
  }, [pid, filePath]);

  useEffect(() => {
    pollRef.current = setInterval(() => {
      if (!dirty) {
        getFile(`${pid}/${filePath}`).then(d => {
          if (!d.error && d.content !== original && d.content !== content) {
            swConfirm('文件已被外部修改', '是否重新加载？').then(yes => {
              if (yes) {
                setContent(d.content);
                setOriginal(d.content);
              }
            });
          }
        });
      }
    }, 3000);
    return () => { if (pollRef.current) clearInterval(pollRef.current); };
  }, [pid, filePath, dirty, original, content]);

  const handleSave = async () => {
    const r = await saveFile(`${pid}/${filePath}`, content);
    if (r.ok) {
      setOriginal(content);
      setDirty(false);
      setSaved(true);
    } else {
      swAlert('保存失败', r.error);
    }
  };

  if (error) return <div style={{ color: 'var(--red)' }}>{error}</div>;

  return (
    <div style={{ display: 'flex', flexDirection: 'column', height: '100%', gap: '4px' }}>
      <div style={{ display: 'flex', gap: '4px', alignItems: 'center' }}>
        <button className="btn" onClick={handleSave} disabled={!dirty}>
          {dirty ? '保存' : saved ? '已保存' : '无变更'}
        </button>
        {dirty && <span style={{ color: 'var(--yellow)', fontSize: 12 }}>未保存</span>}
      </div>
      <CodeEditor
        value={content}
        onChange={v => { setContent(v); setDirty(v !== original); setSaved(false); }}
        language={langForPath(filePath)}
      />
    </div>
  );
}
