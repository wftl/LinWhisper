import { useEffect } from "react";
import { NavLink } from "react-router-dom";
import { useAppStore } from "../stores/appStore";
import StatusIndicator from "./StatusIndicator";
import clsx from "clsx";

interface LayoutProps {
  children: React.ReactNode;
}

export default function Layout({ children }: LayoutProps) {
  const { initialize, isLoading, error, clearError, status, activeMode } =
    useAppStore();

  useEffect(() => {
    initialize();
  }, [initialize]);

  if (isLoading) {
    return (
      <div className="flex items-center justify-center min-h-screen">
        <div className="text-center">
          <div className="animate-spin rounded-full h-12 w-12 border-b-2 border-blue-500 mx-auto mb-4"></div>
          <p className="text-gray-400">Loading WhisperTray...</p>
        </div>
      </div>
    );
  }

  const navItems = [
    { path: "/", label: "Home", icon: "🎤" },
    { path: "/history", label: "History", icon: "📜" },
    { path: "/modes", label: "Modes", icon: "🔧" },
    { path: "/settings", label: "Settings", icon: "⚙️" },
  ];

  return (
    <div className="min-h-screen flex flex-col">
      {/* Header */}
      <header className="bg-gray-800 border-b border-gray-700 px-4 py-3">
        <div className="flex items-center justify-between">
          <div className="flex items-center gap-4">
            <h1 className="text-xl font-semibold text-white">WhisperTray</h1>
            <StatusIndicator status={status} />
          </div>
          {activeMode && (
            <div className="text-sm text-gray-400">
              Mode: <span className="text-white">{activeMode.name}</span>
            </div>
          )}
        </div>
      </header>

      {/* Navigation */}
      <nav className="bg-gray-800 border-b border-gray-700 px-4">
        <div className="flex gap-1">
          {navItems.map((item) => (
            <NavLink
              key={item.path}
              to={item.path}
              className={({ isActive }) =>
                clsx(
                  "px-4 py-2 text-sm font-medium rounded-t transition-colors",
                  isActive
                    ? "bg-gray-700 text-white"
                    : "text-gray-400 hover:text-white hover:bg-gray-700/50"
                )
              }
            >
              <span className="mr-2">{item.icon}</span>
              {item.label}
            </NavLink>
          ))}
        </div>
      </nav>

      {/* Error banner */}
      {error && (
        <div className="bg-red-900/50 border-b border-red-700 px-4 py-2 flex items-center justify-between">
          <span className="text-red-200 text-sm">{error}</span>
          <button
            onClick={clearError}
            className="text-red-300 hover:text-white text-sm"
          >
            Dismiss
          </button>
        </div>
      )}

      {/* Main content */}
      <main className="flex-1 p-4 overflow-auto">{children}</main>

      {/* Footer */}
      <footer className="bg-gray-800 border-t border-gray-700 px-4 py-2 text-center text-xs text-gray-500">
        WinWhisper v0.1.0 • Local-first voice transcription
      </footer>
    </div>
  );
}
