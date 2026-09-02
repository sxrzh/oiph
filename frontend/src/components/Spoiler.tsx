import { useState } from 'react';

export function Spoiler({
  title,
  children,
  defaultOpen = false,
}: {
  title: React.ReactNode;
  children: React.ReactNode;
  defaultOpen?: boolean;
}) {
  const [open, setOpen] = useState(defaultOpen);
  return (
    <div className="spoiler">
      <div className="spoiler-header" onClick={() => setOpen(!open)}>
        <span className="spoiler-arrow">{open ? '▼' : '▶'}</span>
        {title}
      </div>
      {open && <div className="spoiler-body">{children}</div>}
    </div>
  );
}
