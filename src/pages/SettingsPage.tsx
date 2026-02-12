import { useState, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useAppStore } from "../stores/appStore";
import * as api from "../lib/api";

export default function SettingsPage() {
  const { settings, devices, updateSettings, saveApiKey, deleteApiKey } =
    useAppStore();

  const [localSettings, setLocalSettings] = useState(settings);
  const [apiKeys, setApiKeys] = useState({
    openai: "",
    anthropic: "",
  });
  const [hasKeys, setHasKeys] = useState({
    openai: false,
    anthropic: false,
  });
  const [saving, setSaving] = useState(false);
  const [testingConnection, setTestingConnection] = useState(false);
  const [connectionStatus, setConnectionStatus] = useState<"idle" | "success" | "error">("idle");

  useEffect(() => {
    if (settings) {
      setLocalSettings(settings);
    }

    // Check for existing API keys
    Promise.all([api.hasApiKey("openai"), api.hasApiKey("anthropic")]).then(
      ([openai, anthropic]) => {
        setHasKeys({ openai, anthropic });
      }
    );
  }, [settings]);

  const handleSave = async () => {
    if (!localSettings) return;

    setSaving(true);
    try {
      await updateSettings(localSettings);

      // Save API keys if provided
      if (apiKeys.openai) {
        await saveApiKey("openai", apiKeys.openai);
        setHasKeys((prev) => ({ ...prev, openai: true }));
        setApiKeys((prev) => ({ ...prev, openai: "" }));
      }
      if (apiKeys.anthropic) {
        await saveApiKey("anthropic", apiKeys.anthropic);
        setHasKeys((prev) => ({ ...prev, anthropic: true }));
        setApiKeys((prev) => ({ ...prev, anthropic: "" }));
      }
    } finally {
      setSaving(false);
    }
  };

  const handleDeleteKey = async (provider: "openai" | "anthropic") => {
    if (confirm(`Delete ${provider} API key?`)) {
      await deleteApiKey(provider);
      setHasKeys((prev) => ({ ...prev, [provider]: false }));
    }
  };

  const handleTestConnection = async () => {
    const url = localSettings?.whisper_server_url;
    if (!url) return;

    setTestingConnection(true);
    setConnectionStatus("idle");

    try {
      const success = await invoke<boolean>("test_whisper_connection", { url });
      setConnectionStatus(success ? "success" : "error");
    } catch {
      setConnectionStatus("error");
    } finally {
      setTestingConnection(false);
    }
  };

  if (!localSettings) {
    return <div className="text-gray-500">Loading settings...</div>;
  }

  return (
    <div className="max-w-2xl mx-auto space-y-6">
      <h1 className="text-2xl font-semibold text-white">Settings</h1>

      {/* Audio settings */}
      <section className="bg-gray-800 rounded-lg p-4">
        <h2 className="text-lg font-medium text-white mb-4">Audio</h2>

        <div className="space-y-4">
          <div>
            <label className="block text-sm font-medium text-gray-300 mb-1">
              Input Device
            </label>
            <select
              value={localSettings.input_device}
              onChange={(e) =>
                setLocalSettings({ ...localSettings, input_device: e.target.value })
              }
              className="w-full bg-gray-700 border border-gray-600 rounded-lg px-3 py-2 text-white"
            >
              <option value="">Default</option>
              {devices.map((device) => (
                <option key={device.name} value={device.name}>
                  {device.name} {device.is_default && "(System Default)"}
                </option>
              ))}
            </select>
          </div>

          <div>
            <label className="block text-sm font-medium text-gray-300 mb-1">
              Language
            </label>
            <select
              value={localSettings.language}
              onChange={(e) =>
                setLocalSettings({ ...localSettings, language: e.target.value })
              }
              className="w-full bg-gray-700 border border-gray-600 rounded-lg px-3 py-2 text-white"
            >
              <option value="en">English</option>
              <option value="es">Spanish</option>
              <option value="fr">French</option>
              <option value="de">German</option>
              <option value="it">Italian</option>
              <option value="pt">Portuguese</option>
              <option value="ja">Japanese</option>
              <option value="ko">Korean</option>
              <option value="zh">Chinese</option>
            </select>
          </div>
        </div>
      </section>

      {/* STT settings */}
      <section className="bg-gray-800 rounded-lg p-4">
        <h2 className="text-lg font-medium text-white mb-4">
          Speech-to-Text (STT)
        </h2>

        <div className="space-y-4">
          <div>
            <label className="block text-sm font-medium text-gray-300 mb-1">
              Default Provider
            </label>
            <select
              value={localSettings.default_stt_provider}
              onChange={(e) =>
                setLocalSettings({
                  ...localSettings,
                  default_stt_provider: e.target.value,
                })
              }
              className="w-full bg-gray-700 border border-gray-600 rounded-lg px-3 py-2 text-white"
            >
              <option value="whispercpp">whisper.cpp (Local)</option>
              <option value="whisperserver">Self-hosted Whisper Server</option>
              <option value="openai">OpenAI Cloud</option>
              <option value="deepgram">Deepgram</option>
            </select>
          </div>

          {/* Whisper Server URL - shown when whisperserver is selected */}
          {localSettings.default_stt_provider === "whisperserver" && (
            <div>
              <label className="block text-sm font-medium text-gray-300 mb-1">
                Whisper Server URL
                {connectionStatus === "success" && (
                  <span className="ml-2 text-green-400 text-xs">✓ Connected</span>
                )}
                {connectionStatus === "error" && (
                  <span className="ml-2 text-red-400 text-xs">✗ Connection failed</span>
                )}
              </label>
              <div className="flex gap-2">
                <input
                  type="text"
                  value={localSettings.whisper_server_url || ""}
                  onChange={(e) => {
                    setLocalSettings({
                      ...localSettings,
                      whisper_server_url: e.target.value || undefined,
                    });
                    setConnectionStatus("idle");
                  }}
                  placeholder="http://192.168.1.100:8000"
                  className="flex-1 bg-gray-700 border border-gray-600 rounded-lg px-3 py-2 text-white"
                />
                <button
                  onClick={handleTestConnection}
                  disabled={testingConnection || !localSettings.whisper_server_url}
                  className="px-3 py-2 bg-gray-600 text-white rounded-lg text-sm hover:bg-gray-500 disabled:opacity-50"
                >
                  {testingConnection ? "Testing..." : "Test"}
                </button>
              </div>
              <p className="text-xs text-gray-500 mt-1">
                URL of your self-hosted whisper server (Speaches, faster-whisper-server, etc.)
              </p>
            </div>
          )}

          <div>
            <label className="block text-sm font-medium text-gray-300 mb-1">
              Default Model
            </label>
            {localSettings.default_stt_provider === "whisperserver" ? (
              <input
                type="text"
                value={localSettings.default_stt_model}
                onChange={(e) =>
                  setLocalSettings({
                    ...localSettings,
                    default_stt_model: e.target.value,
                  })
                }
                placeholder="e.g., distil-whisper/distil-large-v3.5-ct2"
                className="w-full bg-gray-700 border border-gray-600 rounded-lg px-3 py-2 text-white"
              />
            ) : localSettings.default_stt_provider === "openai" ? (
              <select
                value={localSettings.default_stt_model}
                onChange={(e) =>
                  setLocalSettings({
                    ...localSettings,
                    default_stt_model: e.target.value,
                  })
                }
                className="w-full bg-gray-700 border border-gray-600 rounded-lg px-3 py-2 text-white"
              >
                <option value="gpt-4o-mini-transcribe">gpt-4o-mini-transcribe (fast, cheaper)</option>
                <option value="gpt-4o-transcribe">gpt-4o-transcribe (higher quality)</option>
                <option value="whisper-1">whisper-1 (original)</option>
                <option value="gpt-4o-transcribe-diarize">gpt-4o-transcribe-diarize (speaker labels)</option>
              </select>
            ) : (
              <select
                value={localSettings.default_stt_model}
                onChange={(e) =>
                  setLocalSettings({
                    ...localSettings,
                    default_stt_model: e.target.value,
                  })
                }
                className="w-full bg-gray-700 border border-gray-600 rounded-lg px-3 py-2 text-white"
              >
                <option value="tiny.en">tiny.en (fastest, English only)</option>
                <option value="base.en">base.en (recommended, English only)</option>
                <option value="small.en">small.en (better accuracy)</option>
                <option value="medium.en">medium.en (high accuracy)</option>
                <option value="large-v3">large-v3 (best, multilingual)</option>
              </select>
            )}
            <p className="text-xs text-gray-500 mt-1">
              {localSettings.default_stt_provider === "whisperserver"
                ? "Model name from your server (e.g., check its /v1/models endpoint)"
                : localSettings.default_stt_provider === "openai"
                ? "Diarize adds speaker labels but may need chunking for audio > 30s"
                : "Models are downloaded automatically on first use"}
            </p>
          </div>
        </div>
      </section>

      {/* LLM settings */}
      <section className="bg-gray-800 rounded-lg p-4">
        <h2 className="text-lg font-medium text-white mb-4">
          AI Processing (LLM)
        </h2>

        <div className="space-y-4">
          <div>
            <label className="block text-sm font-medium text-gray-300 mb-1">
              Default Provider
            </label>
            <select
              value={localSettings.default_llm_provider}
              onChange={(e) =>
                setLocalSettings({
                  ...localSettings,
                  default_llm_provider: e.target.value,
                })
              }
              className="w-full bg-gray-700 border border-gray-600 rounded-lg px-3 py-2 text-white"
            >
              <option value="ollama">Ollama (Local)</option>
              <option value="openai">OpenAI</option>
              <option value="anthropic">Anthropic Claude</option>
            </select>
          </div>

          <div>
            <label className="block text-sm font-medium text-gray-300 mb-1">
              Default Model
            </label>
            <input
              type="text"
              value={localSettings.default_llm_model}
              onChange={(e) =>
                setLocalSettings({
                  ...localSettings,
                  default_llm_model: e.target.value,
                })
              }
              placeholder="e.g., llama3.2, gpt-4o, claude-3-sonnet"
              className="w-full bg-gray-700 border border-gray-600 rounded-lg px-3 py-2 text-white"
            />
          </div>
        </div>
      </section>

      {/* API Keys */}
      <section className="bg-gray-800 rounded-lg p-4">
        <h2 className="text-lg font-medium text-white mb-4">API Keys</h2>
        <p className="text-sm text-gray-400 mb-4">
          API keys are stored securely in your system keyring
        </p>

        <div className="space-y-4">
          {/* OpenAI */}
          <div>
            <label className="block text-sm font-medium text-gray-300 mb-1">
              OpenAI API Key
              {hasKeys.openai && (
                <span className="ml-2 text-green-400 text-xs">✓ Configured</span>
              )}
            </label>
            <div className="flex gap-2">
              <input
                type="password"
                value={apiKeys.openai}
                onChange={(e) =>
                  setApiKeys({ ...apiKeys, openai: e.target.value })
                }
                placeholder={hasKeys.openai ? "••••••••" : "sk-..."}
                className="flex-1 bg-gray-700 border border-gray-600 rounded-lg px-3 py-2 text-white"
              />
              {hasKeys.openai && (
                <button
                  onClick={() => handleDeleteKey("openai")}
                  className="px-3 py-2 bg-red-600 text-white rounded-lg text-sm hover:bg-red-700"
                >
                  Delete
                </button>
              )}
            </div>
          </div>

          {/* Anthropic */}
          <div>
            <label className="block text-sm font-medium text-gray-300 mb-1">
              Anthropic API Key
              {hasKeys.anthropic && (
                <span className="ml-2 text-green-400 text-xs">✓ Configured</span>
              )}
            </label>
            <div className="flex gap-2">
              <input
                type="password"
                value={apiKeys.anthropic}
                onChange={(e) =>
                  setApiKeys({ ...apiKeys, anthropic: e.target.value })
                }
                placeholder={hasKeys.anthropic ? "••••••••" : "sk-ant-..."}
                className="flex-1 bg-gray-700 border border-gray-600 rounded-lg px-3 py-2 text-white"
              />
              {hasKeys.anthropic && (
                <button
                  onClick={() => handleDeleteKey("anthropic")}
                  className="px-3 py-2 bg-red-600 text-white rounded-lg text-sm hover:bg-red-700"
                >
                  Delete
                </button>
              )}
            </div>
          </div>
        </div>
      </section>

      {/* Behavior */}
      <section className="bg-gray-800 rounded-lg p-4">
        <h2 className="text-lg font-medium text-white mb-4">Behavior</h2>

        <div className="space-y-4">
          <label className="flex items-center gap-3">
            <input
              type="checkbox"
              checked={localSettings.auto_paste}
              onChange={(e) =>
                setLocalSettings({
                  ...localSettings,
                  auto_paste: e.target.checked,
                })
              }
              className="w-4 h-4 rounded bg-gray-700 border-gray-600 text-blue-600 focus:ring-blue-500"
            />
            <div>
              <span className="text-white">Auto-paste after transcription</span>
              <p className="text-xs text-gray-500">
                Automatically paste the result into the focused application
              </p>
            </div>
          </label>

          <label className="flex items-center gap-3">
            <input
              type="checkbox"
              checked={localSettings.context_awareness}
              onChange={(e) =>
                setLocalSettings({
                  ...localSettings,
                  context_awareness: e.target.checked,
                })
              }
              className="w-4 h-4 rounded bg-gray-700 border-gray-600 text-blue-600 focus:ring-blue-500"
            />
            <div>
              <span className="text-white">Context awareness</span>
              <p className="text-xs text-gray-500">
                Include clipboard content as context for AI processing
              </p>
            </div>
          </label>
        </div>
      </section>

      {/* Save button */}
      <div className="flex justify-end">
        <button
          onClick={handleSave}
          disabled={saving}
          className="px-6 py-2 bg-blue-600 text-white rounded-lg font-medium hover:bg-blue-700 disabled:opacity-50"
        >
          {saving ? "Saving..." : "Save Settings"}
        </button>
      </div>
    </div>
  );
}
