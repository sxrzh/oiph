// 深浅色主题：localStorage 持久化 + <html data-theme> 切换，默认浅色。

export type Theme = 'light' | 'dark';

const THEME_KEY = 'oiph-theme';

export function getTheme(): Theme {
  try {
    return localStorage.getItem(THEME_KEY) === 'dark' ? 'dark' : 'light';
  } catch {
    return 'light';
  }
}

export function applyTheme(theme: Theme) {
  document.documentElement.setAttribute('data-theme', theme);
}

/** 应用已保存主题（入口调用）。返回当前主题。 */
export function initTheme(): Theme {
  const t = getTheme();
  applyTheme(t);
  return t;
}

/** 保存并立即应用主题。 */
export function saveTheme(theme: Theme) {
  try {
    localStorage.setItem(THEME_KEY, theme);
  } catch {
    // localStorage 不可用时仅当前页面生效
  }
  applyTheme(theme);
}

/** 监听其他标签页的主题切换（设置页与主界面是两个页面）。 */
export function watchTheme() {
  window.addEventListener('storage', e => {
    if (e.key === THEME_KEY) applyTheme(getTheme());
  });
}
