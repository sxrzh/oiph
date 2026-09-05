import { useEffect, useRef, useState } from 'react';
import ReactMarkdown from 'react-markdown';
import remarkMath from 'remark-math';
import rehypeKatex from 'rehype-katex';
import { Spoiler } from './Spoiler';

interface DisplayMessage {
  role: string;
  content: string;
  toolCalls?: string;
  agent?: string;
  toolId?: number;
  running?: boolean;
  startedAt?: number;
  toolName?: string;
}

interface ChildSession {
  filename: string;
  agent: string;
  summary: string;
}

interface SubMessage {
  role: string;
  content: string | null;
}

export function ChatArea({
  messages,
  childSessions,
  sessionName,
  streaming,
  onSend,
  onStop,
  onUndo,
  onRedo,
  children: questionnaire,
}: {
  messages: DisplayMessage[];
  childSessions: ChildSession[];
  sessionName: string | null;
  streaming: boolean;
  onSend: (text: string) => void;
  onStop: () => void;
  onUndo: () => void;
  onRedo: () => void;
  children?: React.ReactNode;
}) {
  const [input, setInput] = useState('');
  const chatRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (chatRef.current) chatRef.current.scrollTop = chatRef.current.scrollHeight;
  }, [messages]);

  const handleSend = () => {
    if (!input.trim()) return;
    onSend(input.trim());
    setInput('');
  };

  // Track which tool result message corresponds to which child session
  let childIndex = 0;

  return (
    <>
      <div className="chat-area" ref={chatRef}>
        {messages.map((msg, i) => {
          const isSubAgentResult = msg.role === 'tool' && msg.content.includes('[sub-session]');
          let child: ChildSession | null = null;
          if (isSubAgentResult && childIndex < childSessions.length) {
            child = childSessions[childIndex++];
          }
          // 思维链消息用 spoiler 包裹
          if (msg.role === 'reasoning') {
            return <ReasoningMessageView key={i} content={msg.content} agent={msg.agent} />;
          }
          return <ChatMessageView key={i} msg={msg} child={child} sessionName={sessionName} />;
        })}
      </div>
      {questionnaire}
      <div className="input-area">
        <textarea
          className="chat-input"
          value={input}
          onChange={e => setInput(e.target.value)}
          onKeyDown={e => {
            if (e.key === 'Enter' && !e.shiftKey) { e.preventDefault(); handleSend(); }
          }}
          placeholder="输入消息... (Enter 发送, Shift+Enter 换行)"
          rows={1}
        />
        {streaming ? (
          <button className="btn-stop" onClick={onStop}>中止</button>
        ) : (
          <>
            <button className="btn" onClick={onUndo} title="回滚工作区到上一个快照">↩ Undo</button>
            <button className="btn" onClick={onRedo} title="恢复回滚前的状态">↪ Redo</button>
            <button className="btn-send" onClick={handleSend}>发送</button>
          </>
        )}
      </div>
    </>
  );
}

function ReasoningMessageView({ content, agent }: { content: string; agent?: string }) {
  return (
    <div className="msg reasoning" style={{ background: 'var(--bg-tab)', maxWidth: '85%', alignSelf: 'flex-start' }}>
      <Spoiler title={`🧠 ${agent ?? 'agent'} 的思维链（${content.length} 字符）`} defaultOpen={false}>
        <div style={{ whiteSpace: 'pre-wrap', fontSize: 12, color: 'var(--fg-muted)', maxHeight: '400px', overflow: 'auto' }}>
          {content}
        </div>
      </Spoiler>
    </div>
  );
}

/// 运行中工具的计时徽标（动态渐变特效）。
function RunningToolBadge({ name, startedAt }: { name: string; startedAt: number }) {
  const [sec, setSec] = useState(() => Math.max(0, Math.floor((Date.now() - startedAt) / 1000)));
  useEffect(() => {
    const t = setInterval(() => {
      setSec(Math.max(0, Math.floor((Date.now() - startedAt) / 1000)));
    }, 1000);
    return () => clearInterval(t);
  }, [startedAt]);
  return (
    <span className="tool-running-badge">
      <span className="tool-running-dot" />
      ⚙ {name} · 工具已运行 {sec} 秒
    </span>
  );
}

function ChatMessageView({
  msg,
  child,
  sessionName,
}: {
  msg: DisplayMessage;
  child: ChildSession | null;
  sessionName: string | null;
}) {
  const roleTag = msg.role === 'user' ? '你' :
    msg.role === 'assistant' ? 'Supervisor' :
    msg.role === 'tool' ? '工具' :
    msg.role === 'system' ? '系统' : msg.role;
  const displayContent = msg.content.replace('[sub-session]', '').trim();

  // 只有 agent 的对话渲染 Markdown（含 LaTeX 公式），工具调用/用户消息/思维链纯文本
  const body = msg.role === 'assistant' ? (
    <div className="md-body">
      <ReactMarkdown remarkPlugins={[remarkMath]} rehypePlugins={[rehypeKatex]}>
        {displayContent}
      </ReactMarkdown>
    </div>
  ) : (
    <div className="text" style={{ whiteSpace: 'pre-wrap' }}>{displayContent}</div>
  );

  return (
    <div className={`msg ${msg.role}${msg.running ? ' msg-running' : ''}`}>
      <div className="role-tag">{roleTag}</div>
      {body}
      {msg.toolCalls && <div className="tool-call" style={{ marginTop: '4px' }}>{msg.toolCalls}</div>}
      {msg.running && msg.toolName && msg.startedAt != null && (
        <div style={{ marginTop: '6px' }}>
          <RunningToolBadge name={msg.toolName} startedAt={msg.startedAt} />
        </div>
      )}
      {child && sessionName && (
        <SubSessionSpoiler child={child} sessionName={sessionName} />
      )}
    </div>
  );
}

function SubSessionSpoiler({ child, sessionName }: { child: ChildSession; sessionName: string }) {
  const [loaded, setLoaded] = useState(false);
  const [messages, setMessages] = useState<SubMessage[]>([]);

  const loadSub = async () => {
    if (loaded) return;
    try {
      const r = await fetch(`/api/session/sub?session=${encodeURIComponent(sessionName)}&filename=${encodeURIComponent(child.filename)}`);
      const d = await r.json();
      if (d.messages) setMessages(d.messages);
      setLoaded(true);
    } catch {
      setLoaded(true);
    }
  };

  return (
    <Spoiler title={`📋 ${child.agent} agent 对话记录`} defaultOpen={false}>
      <div onClick={loadSub} style={{ cursor: loaded ? 'default' : 'pointer' }}>
        {!loaded && <p style={{ color: 'var(--fg-muted)', fontSize: 12 }}>点击加载子 agent 对话...</p>}
        {loaded && messages.length === 0 && <p style={{ color: 'var(--fg-muted)' }}>（无消息）</p>}
        {loaded && messages.map((m, i) => (
          <div key={i} style={{ margin: '4px 0', padding: '4px 8px', background: 'var(--bg)', borderRadius: 4, fontSize: 12 }}>
            <span style={{ color: 'var(--fg-muted)' }}>{m.role}: </span>
            <span style={{ whiteSpace: 'pre-wrap' }}>{m.content || ''}</span>
          </div>
        ))}
      </div>
    </Spoiler>
  );
}
