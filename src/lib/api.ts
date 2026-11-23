import { invoke } from "@tauri-apps/api/core";
import type {
  Mode,
  AudioDevice,
  HistoryItem,
  Settings,
  RecordingStatusResponse,
  HistoryQuery,
  ExportFormat,
} from "../types";

// Recording
export async function startRecording(): Promise<void> {
  return invoke("start_recording");
}

export async function stopRecording(): Promise<string> {
  return invoke("stop_recording");
}

export async function getRecordingStatus(): Promise<RecordingStatusResponse> {
  return invoke("get_recording_status");
}

// Modes
export async function getModes(): Promise<Mode[]> {
  return invoke("get_modes");
}

export async function setActiveMode(modeKey: string): Promise<void> {
  return invoke("set_active_mode", { modeKey });
}

export async function getActiveMode(): Promise<Mode | null> {
  return invoke("get_active_mode");
}

// Devices
export async function getInputDevices(): Promise<AudioDevice[]> {
  return invoke("get_input_devices");
}

export async function setInputDevice(deviceName: string): Promise<void> {
  return invoke("set_input_device", { deviceName });
}

// File transcription
export async function transcribeFile(filePath: string): Promise<string> {
  return invoke("transcribe_file", { filePath });
}

// History
export async function getHistory(query?: HistoryQuery): Promise<HistoryItem[]> {
  return invoke("get_history", { query });
}

export async function getHistoryItem(id: string): Promise<HistoryItem | null> {
  return invoke("get_history_item", { id });
}

export async function reprocessHistoryItem(
  id: string,
  modeKey: string
): Promise<string> {
  return invoke("reprocess_history_item", { id, modeKey });
}

export async function deleteHistoryItem(id: string): Promise<void> {
  return invoke("delete_history_item", { id });
}

export async function exportHistoryItem(
  id: string,
  format: ExportFormat
): Promise<string> {
  return invoke("export_history_item", { id, format });
}

// Settings
export async function getSettings(): Promise<Settings> {
  return invoke("get_settings");
}

export async function updateSettings(settings: Settings): Promise<void> {
  return invoke("update_settings", { settings });
}

export async function preloadSttModel(
  provider: string,
  model: string
): Promise<void> {
  return invoke("preload_stt_model", { provider, model });
}

// API Keys
export async function saveApiKey(provider: string, key: string): Promise<void> {
  return invoke("save_api_key", { provider, key });
}

export async function deleteApiKey(provider: string): Promise<void> {
  return invoke("delete_api_key", { provider });
}

export async function hasApiKey(provider: string): Promise<boolean> {
  return invoke("has_api_key", { provider });
}
