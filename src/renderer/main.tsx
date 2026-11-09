import React from 'react';
import ReactDOM from 'react-dom/client';
import { getCurrentWindow } from '@tauri-apps/api/window';
import App from './App';
import ComposeWindowApp from './ComposeWindowApp';
import { isComposeWindowLabel } from './services/composeWindow';
import './index.css';
import './i18n';

// 作成の別ウィンドウ（label `compose-<連番>`）は作成画面だけを出す（docs/COMPOSE.md §別ウィンドウ）。
const isComposeWindow =
  '__TAURI_INTERNALS__' in window && isComposeWindowLabel(getCurrentWindow().label);

ReactDOM.createRoot(document.getElementById('root') as HTMLElement).render(
  <React.StrictMode>{isComposeWindow ? <ComposeWindowApp /> : <App />}</React.StrictMode>
);
