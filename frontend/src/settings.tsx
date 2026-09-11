import { createRoot } from 'react-dom/client';
import './index.css';
import { SettingsPage } from './SettingsPage';
import { initTheme, watchTheme } from './theme';

initTheme();
watchTheme();

createRoot(document.getElementById('root')!).render(<SettingsPage />);
