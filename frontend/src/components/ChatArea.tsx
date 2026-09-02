import { useEffect, useRef, useState } from 'react';

interface DisplayMessage {
  role: string;
  content: string;
  toolCalls?: string;
  reasoning?: string;
}

export function ChatArea({
  messages,
  streaming,
  onSend,
  onStop,
}: {
  messages: DisplayMessage[];
  streaming: boolean;
  onSend: (text: string) => void;
  onStop: () => void;
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

  return (
    <>
      <div className="chat-area" ref={chatRef}>
        {messages.map((msg, i) => (
          <ChatMessageView key={i} msg={msg} />
        ))}
      </div>
      <div className="input-area">
        <textarea
          className="chat-input"
          value={input}
          onChange={e => setInput(e.target.value)}
          onKeyDown={e => {
            if (e.key === 'Enter' && !e.shiftKey) {
              e.preventDefault();
              handleSend();
            }
          }}
          placeholder="输入消息... (Enter 发送, Shift+Enter 换行)"
          rows={1}
        />
        {streaming ? (
          <button className="btn-stop" onClick={onStop}>中止</button>
        ) : (
          <button className="btn-send" onClick={handleSend}>发送</button>
        )}
      </div>
    </>
  );
}

function ChatMessageView({ msg }: { msg: DisplayMessage }) {
  const roleTag = msg.role === 'user' ? '你' :
    msg.role === 'assistant' ? 'Supervisor' :
    msg.role === 'tool' ? '工具' :
    msg.role === 'reasoning' ? '思维链' : msg.role;

  return (
    <div className={`msg ${msg.role}`}>
      <div className="role-tag">{roleTag}</div>
      <div className="text">
        {msg.content}
        {msg.toolCalls && <div className="tool-call" style={{ marginTop: '4px' }}>{msg.toolCalls}</div>}
      </div>
    </div>
  );
}
