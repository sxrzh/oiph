import { useEffect, useRef, useState } from 'react';
import ReactMarkdown from 'react-markdown';
import remarkMath from 'remark-math';
import rehypeKatex from 'rehype-katex';
import { getFile, saveFile } from '../api';

export function MarkdownEditor({ pid, filePath }: { pid: string; filePath: string }) {
  const [content, setContent] = useState('');
  const [original, setOriginal] = useState('');
  const [dirty, setDirty] = useState(false);
  const [showPreview, setShowPreview] = useState(true);
  const [saved, setSaved] = useState(false);
  const pollRef = useRef<ReturnType<typeof setInterval> | null>(null);

  // Load file
  useEffect(() => {
    setDirty(false);
    getFile(`${pid}/${filePath}`).then(d => {
      if (d.error) {
        setContent(`（文件不存在：${filePath}）`);
        setOriginal('');
      } else {
        setContent(d.content);
        setOriginal(d.content);
      }
    });
    return () => {
      if (pollRef.current) clearInterval(pollRef.current);
    };
  }, [pid, filePath]);

  // Poll for external file changes
  useEffect(() => {
    pollRef.current = setInterval(() => {
      if (!dirty) {
        getFile(`${pid}/${filePath}`).then(d => {
          if (!d.error && d.content !== original && d.content !== content) {
            if (confirm('文件已被外部修改，是否重新加载？')) {
              setContent(d.content);
              setOriginal(d.content);
            }
          }
        });
      }
    }, 3000);
    return () => { if (pollRef.current) clearInterval(pollRef.current); };
  }, [pid, filePath, dirty, original, content]);

  const handleChange = (v: string) => {
    setContent(v);
    setDirty(v !== original);
    setSaved(false);
    // Debounce: nothing to debounce for editing, only preview is debounced
  };

  const handleSave = async () => {
    const r = await saveFile(`${pid}/${filePath}`, content);
    if (r.ok) {
      setOriginal(content);
      setDirty(false);
      setSaved(true);
    } else {
      alert(r.error);
    }
  };

  return (
    <div style={{ display: 'flex', flexDirection: 'column', height: '100%', gap: '4px' }}>
      <div style={{ display: 'flex', gap: '4px', alignItems: 'center' }}>
        <button className="btn" onClick={handleSave} disabled={!dirty}>
          {dirty ? '保存' : saved ? '已保存' : '无变更'}
        </button>
        <button className="btn" onClick={() => setShowPreview(!showPreview)}>
          {showPreview ? '隐藏预览' : '显示预览'}
        </button>
        {dirty && <span style={{ color: 'var(--yellow)', fontSize: 12 }}>未保存</span>}
      </div>
      <div style={{ display: 'flex', gap: '8px', flex: 1, minHeight: 0 }}>
        <textarea
          className="editor"
          style={{ flex: 1 }}
          value={content}
          onChange={e => handleChange(e.target.value)}
          placeholder="输入 Markdown..."
        />
        {showPreview && (
          <div className="markdown-preview" style={{ flex: 1, overflow: 'auto' }}>
            <DebouncedPreview content={content} />
          </div>
        )}
      </div>
    </div>
  );
}

function DebouncedPreview({ content }: { content: string }) {
  const [debounced, setDebounced] = useState(content);
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null);

  useEffect(() => {
    if (timer.current) clearTimeout(timer.current);
    timer.current = setTimeout(() => setDebounced(content), 500);
    return () => { if (timer.current) clearTimeout(timer.current); };
  }, [content]);

  return (
    <ReactMarkdown remarkPlugins={[remarkMath]} rehypePlugins={[rehypeKatex]}>
      {debounced}
    </ReactMarkdown>
  );
}
