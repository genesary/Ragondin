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
    {/* The page follows /jobs/events once, for the top bar, every screen that shows the queue, and the toasts (ARCHITECTURE.md § The job stream). */}
    <App client={createApiClient()} build={BUILD} reload={() => window.location.reload()} followJobs />
  </StrictMode>,
);
