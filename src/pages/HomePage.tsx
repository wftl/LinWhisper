import { useAppStore } from "../stores/appStore";
import clsx from "clsx";

export default function HomePage() {
  const {
    status,
    isRecording,
    lastOutput,
    activeMode,
    modes,
    startRecording,
    stopRecording,
    setActiveMode,
  } = useAppStore();

  const handleRecordClick = () => {
    if (isRecording) {
      stopRecording();
    } else {
      startRecording();
    }
  };

  return (
    <div className="max-w-2xl mx-auto space-y-6">
      {/* Recording button */}
      <div className="text-center">
        <button
          onClick={handleRecordClick}
          disabled={status === "processing" || status === "loading"}
          className={clsx(
            "w-32 h-32 rounded-full text-white font-semibold text-lg",
            "transition-all duration-150",
            "focus:outline-none focus:ring-4",
            "active:scale-95 active:brightness-90",
            isRecording
              ? "bg-red-600 hover:bg-red-700 focus:ring-red-500/50 animate-pulse"
              : status === "processing"
              ? "bg-blue-600 cursor-not-allowed"
              : status === "loading"
              ? "bg-yellow-600 cursor-not-allowed"
              : "bg-green-600 hover:bg-green-700 focus:ring-green-500/50"
          )}
        >
          {isRecording
            ? "Stop"
            : status === "processing"
            ? "..."
            : status === "loading"
            ? "Loading"
            : "Record"}
        </button>
        <p className="mt-4 text-gray-400 text-sm">
          {isRecording
            ? "Click to stop recording"
            : "Click to start recording, or use the tray icon"}
        </p>
      </div>

      {/* Mode selector */}
      <div className="bg-gray-800 rounded-lg p-4">
        <label className="block text-sm font-medium text-gray-300 mb-2">
          Active Mode
        </label>
        <select
          value={activeMode?.key || ""}
          onChange={(e) => setActiveMode(e.target.value)}
          className="w-full bg-gray-700 border border-gray-600 rounded-lg px-3 py-2 text-white focus:outline-none focus:ring-2 focus:ring-blue-500"
        >
          {modes.map((mode) => (
            <option key={mode.key} value={mode.key}>
              {mode.name} - {mode.description}
            </option>
          ))}
        </select>
        {activeMode && (
          <p className="mt-2 text-sm text-gray-400">
            {activeMode.ai_processing
              ? `AI processing enabled (${activeMode.llm_provider})`
              : "Direct transcription (no AI processing)"}
          </p>
        )}
      </div>

      {/* Last output */}
      {lastOutput && (
        <div className="bg-gray-800 rounded-lg p-4">
          <div className="flex items-center justify-between mb-2">
            <h3 className="text-sm font-medium text-gray-300">Last Output</h3>
            <button
              onClick={() => navigator.clipboard.writeText(lastOutput)}
              className="text-xs text-blue-400 hover:text-blue-300"
            >
              Copy
            </button>
          </div>
          <div className="bg-gray-900 rounded p-3 text-sm text-gray-100 whitespace-pre-wrap max-h-64 overflow-auto">
            {lastOutput}
          </div>
        </div>
      )}

      {/* Quick tips */}
      <div className="bg-gray-800/50 rounded-lg p-4 text-sm text-gray-400">
        <h3 className="font-medium text-gray-300 mb-2">Quick Tips</h3>
        <ul className="list-disc list-inside space-y-1">
          <li><kbd className="px-1 py-0.5 rounded bg-gray-700 text-gray-200 text-xs font-mono">Ctrl+Space</kbd> to toggle recording from anywhere</li>
          <li>Left-click the tray icon to toggle recording</li>
          <li>Right-click the tray icon for the menu</li>
          <li>Transcriptions are automatically copied to clipboard</li>
          <li>Enable "auto paste" in settings to paste directly</li>
        </ul>
      </div>
    </div>
  );
}
