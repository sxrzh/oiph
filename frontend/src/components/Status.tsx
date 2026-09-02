import type { ComponentStatus } from '../types';

export function StatusDot({ status }: { status: ComponentStatus | null }) {
  if (!status) return <span className="status-dot dot-gray" />;
  const color = `dot-${status.color}`;
  return <span className={`status-dot ${color}`} title={status.label} />;
}

export function StatusCell({ status }: { status: ComponentStatus | null }) {
  if (!status) return <span className="status-cell gray">-</span>;
  return (
    <span className={`status-cell ${status.color}`} title={status.error || status.message || status.timestamp || ''}>
      {status.label}
    </span>
  );
}

export function ProgressRing({ status }: { status: ComponentStatus | null }) {
  const color = status?.color ?? 'gray';
  const filled = status?.state === 'completed';
  return (
    <span
      className={`progress-ring ${filled ? 'filled' : ''}`}
      style={{ borderColor: `var(--${color})` }}
      title={status?.label ?? ''}
    />
  );
}
