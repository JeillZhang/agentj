import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
import "@agentj/tokens/tokens.css";
import "./index.css";
import "./App.css";
import "./components/layout.css";
import "./components/filetree.css";
import "./components/options.css";
import "./components/ask.css";
import "./components/topbarmenu.css";
import "./components/split.css";
import "./components/rightpanel.css";
import "./components/sidebar.css";
import "./components/connection.css";
import "./components/session.css";
import "./components/composer.css";
import "./components/conversation.css";
import "./components/trajectory.css";
import "./components/hero.css";
import "./components/dialog.css";
import "./components/displayPrefs.css";
import App from './App.tsx'

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <App />
  </StrictMode>,
)
