import { create } from "zustand";
import { listen } from "@tauri-apps/api/event";
import * as api from "../lib/api";
import type {
  Mode,
  AudioDevice,
  HistoryItem,
  Settings,
  RecordingStatus,
} from "../types";

interface AppState {
  // Recording state
  status: RecordingStatus;
  isRecording: boolean;
  lastOutput: string | null;

  // Modes
  modes: Mode[];
  activeMode: Mode | null;

  // Devices
  devices: AudioDevice[];
  selectedDevice: string;

  // History
  history: HistoryItem[];
  selectedHistoryItem: HistoryItem | null;

  // Settings
  settings: Settings | null;

  // Loading states
  isLoading: boolean;
  error: string | null;

  // Actions
  initialize: () => Promise<void>;
  startRecording: () => Promise<void>;
  stopRecording: () => Promise<void>;
  setActiveMode: (modeKey: string) => Promise<void>;
  setInputDevice: (deviceName: string) => Promise<void>;
  loadHistory: (search?: string) => Promise<void>;
  selectHistoryItem: (item: HistoryItem | null) => void;
  reprocessHistoryItem: (id: string, modeKey: string) => Promise<void>;
  deleteHistoryItem: (id: string) => Promise<void>;
  updateSettings: (settings: Settings) => Promise<void>;
  saveApiKey: (provider: string, key: string) => Promise<void>;
  deleteApiKey: (provider: string) => Promise<void>;
  clearError: () => void;
}

export const useAppStore = create<AppState>((set, get) => ({
  // Initial state
  status: "loading",
  isRecording: false,
  lastOutput: null,
  modes: [],
  activeMode: null,
  devices: [],
  selectedDevice: "",
  history: [],
  selectedHistoryItem: null,
  settings: null,
  isLoading: true,
  error: null,

  // Initialize the app
  initialize: async () => {
    try {
      set({ isLoading: true, error: null });

      // Load all data in parallel
      const [modes, activeMode, devices, settings, statusResponse] =
        await Promise.all([
          api.getModes(),
          api.getActiveMode(),
          api.getInputDevices(),
          api.getSettings(),
          api.getRecordingStatus(),
        ]);

      set({
        modes,
        activeMode,
        devices,
        settings,
        selectedDevice: settings.input_device,
        status: statusResponse.status,
        isRecording: statusResponse.is_recording,
        isLoading: false,
      });

      // Set up event listeners
      listen<string>("recording-complete", (event) => {
        set({
          status: "ready",
          isRecording: false,
          lastOutput: event.payload,
        });
        // Refresh history
        get().loadHistory();
      });

      listen<string>("recording-error", (event) => {
        set({
          status: "ready",
          isRecording: false,
          error: event.payload,
        });
      });

      listen("recording-started", () => {
        set({ status: "recording", isRecording: true });
      });
    } catch (error) {
      set({
        error: error instanceof Error ? error.message : "Failed to initialize",
        isLoading: false,
      });
    }
  },

  // Start recording
  startRecording: async () => {
    try {
      set({ error: null });
      await api.startRecording();
      set({ status: "recording", isRecording: true });
    } catch (error) {
      set({
        error: error instanceof Error ? error.message : "Failed to start recording",
        status: "error",
      });
    }
  },

  // Stop recording
  stopRecording: async () => {
    try {
      set({ status: "processing", error: null });
      const output = await api.stopRecording();
      set({
        status: "ready",
        isRecording: false,
        lastOutput: output,
      });
      // Refresh history
      get().loadHistory();
    } catch (error) {
      // Re-fetch backend state to ensure sync
      const statusResponse = await api.getRecordingStatus().catch(() => null);
      set({
        error: error instanceof Error ? error.message : "Failed to stop recording",
        status: statusResponse?.status ?? "ready",
        isRecording: statusResponse?.is_recording ?? false,
      });
    }
  },

  // Set active mode
  setActiveMode: async (modeKey: string) => {
    try {
      set({ error: null });
      await api.setActiveMode(modeKey);
      const activeMode = get().modes.find((m) => m.key === modeKey) || null;
      set({ activeMode });
    } catch (error) {
      set({
        error: error instanceof Error ? error.message : "Failed to set mode",
      });
    }
  },

  // Set input device
  setInputDevice: async (deviceName: string) => {
    try {
      set({ error: null });
      await api.setInputDevice(deviceName);
      set({ selectedDevice: deviceName });
    } catch (error) {
      set({
        error: error instanceof Error ? error.message : "Failed to set device",
      });
    }
  },

  // Load history
  loadHistory: async (search?: string) => {
    try {
      set({ error: null });
      const history = await api.getHistory({ search, limit: 100 });
      set({ history });
    } catch (error) {
      set({
        error: error instanceof Error ? error.message : "Failed to load history",
      });
    }
  },

  // Select history item
  selectHistoryItem: (item: HistoryItem | null) => {
    set({ selectedHistoryItem: item });
  },

  // Reprocess history item
  reprocessHistoryItem: async (id: string, modeKey: string) => {
    try {
      set({ error: null, status: "processing" });
      await api.reprocessHistoryItem(id, modeKey);
      set({ status: "ready" });
      // Refresh history
      get().loadHistory();
    } catch (error) {
      set({
        error: error instanceof Error ? error.message : "Failed to reprocess",
        status: "ready",
      });
    }
  },

  // Delete history item
  deleteHistoryItem: async (id: string) => {
    try {
      set({ error: null });
      await api.deleteHistoryItem(id);
      // Remove from local state
      set((state) => ({
        history: state.history.filter((h) => h.id !== id),
        selectedHistoryItem:
          state.selectedHistoryItem?.id === id ? null : state.selectedHistoryItem,
      }));
    } catch (error) {
      set({
        error: error instanceof Error ? error.message : "Failed to delete",
      });
    }
  },

  // Update settings
  updateSettings: async (settings: Settings) => {
    try {
      set({ error: null });
      await api.updateSettings(settings);
      set({ settings });
    } catch (error) {
      set({
        error: error instanceof Error ? error.message : "Failed to save settings",
      });
    }
  },

  // Save API key
  saveApiKey: async (provider: string, key: string) => {
    try {
      set({ error: null });
      await api.saveApiKey(provider, key);
    } catch (error) {
      set({
        error: error instanceof Error ? error.message : "Failed to save API key",
      });
    }
  },

  // Delete API key
  deleteApiKey: async (provider: string) => {
    try {
      set({ error: null });
      await api.deleteApiKey(provider);
    } catch (error) {
      set({
        error: error instanceof Error ? error.message : "Failed to delete API key",
      });
    }
  },

  // Clear error
  clearError: () => {
    set({ error: null });
  },
}));
