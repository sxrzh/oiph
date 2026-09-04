import Swal from 'sweetalert2';

/** 弹窗提示（替代 alert）。 */
export function swAlert(title: string, text?: string) {
  return Swal.fire({
    title,
    text,
    background: 'var(--bg-panel)',
    color: 'var(--fg)',
    confirmButtonColor: '#e94560',
    customClass: { popup: 'swal-dark' },
  });
}

/** 弹窗确认（替代 confirm），返回是否确认。 */
export function swConfirm(title: string, text?: string): Promise<boolean> {
  return Swal.fire({
    title,
    text,
    showCancelButton: true,
    confirmButtonText: '确定',
    cancelButtonText: '取消',
    background: 'var(--bg-panel)',
    color: 'var(--fg)',
    confirmButtonColor: '#e94560',
    cancelButtonColor: '#555',
  }).then(r => r.isConfirmed);
}

/** 弹窗错误提示。 */
export function swError(title: string, text?: string) {
  return Swal.fire({
    icon: 'error',
    title,
    text,
    background: 'var(--bg-panel)',
    color: 'var(--fg)',
    confirmButtonColor: '#e94560',
  });
}
