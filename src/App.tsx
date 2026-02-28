import { useEffect } from "react";
import { Routes, Route, useNavigate, useLocation } from "react-router-dom";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import Layout from "./components/Layout";
import HomePage from "./pages/HomePage";
import HistoryPage from "./pages/HistoryPage";
import SettingsPage from "./pages/SettingsPage";
import ModesPage from "./pages/ModesPage";
import RecordingIndicator from "./pages/RecordingIndicator";

// Resolved once at module load — the label never changes for a given window.
const WINDOW_LABEL = getCurrentWebviewWindow().label;

function App() {
  const navigate = useNavigate();
  const location = useLocation();

  useEffect(() => {
    // The recording indicator window must never respond to navigate events —
    // guard by window label so it stays on /recording regardless of what
    // the backend emits (belt-and-suspenders on top of emit_to("main", …)).
    if (WINDOW_LABEL === "recording") return;

    const unlisten = listen<string>("navigate", (event) => {
      navigate(event.payload);
    });

    return () => {
      unlisten.then((fn) => fn());
    };
  }, [navigate]);

  // Recording indicator window: always render by label, not just by pathname,
  // so a stale route can never cause it to accidentally render the main layout.
  if (WINDOW_LABEL === "recording" || location.pathname === "/recording") {
    return <RecordingIndicator />;
  }

  return (
    <Layout>
      <Routes>
        <Route path="/" element={<HomePage />} />
        <Route path="/history" element={<HistoryPage />} />
        <Route path="/settings" element={<SettingsPage />} />
        <Route path="/modes" element={<ModesPage />} />
      </Routes>
    </Layout>
  );
}

export default App;
