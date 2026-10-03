import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
import '../design/base.css';
import { createApiClient } from './api/client.ts';
import { App } from './App.tsx';
import { BUILD } from './shell/build.ts';

const root = document.getElementById('root');
if (root === null) throw new Error('index.html has no #root element');

createRoot(root).render(
  <StrictMode>
    {/* No stream for the top bar's connection state: Setup follows /jobs/events itself (ARCHITECTURE.md § The job stream). */}
    <App client={createApiClient()} build={BUILD} reload={() => window.location.reload()} />
  </StrictMode>,
);
