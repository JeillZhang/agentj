import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
import "@agentj/tokens/tokens.css";
import "./index.css";
import "./App.css";
import App from './App.tsx'

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <App />
  </StrictMode>,
)
