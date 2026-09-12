// 深浅色主题：持久化在服务器端（~/.oiph/config/ui.json），
// localStorage 仅作防闪烁缓存；配置不存在时默认浅色（兼容旧安装）。

export type Theme = 'light' | 'dark';

const THEME_KEY = 'oiph-theme';

/** 本地缓存的主题（用于首屏立即应用，避免闪烁）。 */
export function getCachedTheme(): Theme {
  try {
    return localStorage.getItem(THEME_KEY) === 'dark' ? 'dark' : 'light';
  } catch {
    return 'light';
  }
}

export function applyTheme(theme: Theme) {
  document.documentElement.setAttribute('data-theme', theme);
}

function cacheTheme(theme: Theme) {
  try {
    localStorage.setItem(THEME_KEY, theme);
  } catch {
    // 隐私模式等场景忽略
  }
}

/** 从服务器读取主题；网络失败返回 null（保持当前/缓存值）。 */
export async function fetchTheme(): Promise<Theme | null> {
  try {
    const r = await fetch('/api/settings/ui');
    const d = await r.json();
    if (d.error) return null;
    return d.theme === 'dark' ? 'dark' : 'light';
  } catch {
    return null;
  }
}

/**
 * 入口调用：先用本地缓存立即应用（防闪烁），再与服务器同步。
 * 服务器返回后覆盖本地缓存（多浏览器一致）。
 */
export async function initTheme(): Promise<Theme> {
  const cached = getCachedTheme();
  applyTheme(cached);
  const server = await fetchTheme();
  if (server) {
    applyTheme(server);
    cacheTheme(server);
    return server;
  }
  return cached;
}

/** 保存主题：写入服务器 config，同时立即应用并更新本地缓存。 */
export async function saveTheme(theme: Theme): Promise<boolean> {
  applyTheme(theme);
  cacheTheme(theme);
  try {
    const r = await fetch('/api/settings/ui', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ theme }),
    });
    const d = await r.json();
    return !d.error;
  } catch {
    return false;
  }
}

/** 监听其他标签页的主题切换（设置页与主界面是两个页面）。 */
export function watchTheme() {
  window.addEventListener('storage', e => {
    if (e.key === THEME_KEY) applyTheme(getCachedTheme());
  });
}
