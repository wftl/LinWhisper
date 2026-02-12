// Recording status
export type RecordingStatus = "loading" | "recording" | "processing" | "ready" | "error";

// STT provider types
export type SttProvider = "whispercpp" | "whisperserver" | "openai" | "deepgram" | string;

// LLM provider types
export type LlmProvider = "openai" | "anthropic" | "ollama" | string;

// Output format
export type OutputFormat = "plain" | "markdown";

// Mode definition
export interface Mode {
  key: string;
  name: string;
  description: string;
  stt_provider: SttProvider;
  stt_model: string;
  ai_processing: boolean;
  llm_provider: LlmProvider;
  llm_model: string;
  prompt_template: string;
  output_format: OutputFormat;
  builtin: boolean;
}

// Audio device
export interface AudioDevice {
  name: string;
  is_default: boolean;
}

// History item
export interface HistoryItem {
  id: string;
  created_at: string;
  mode_key: string;
  audio_path: string | null;
  transcript_raw: string;
  output_final: string;
  stt_provider: string;
  stt_model: string;
  llm_provider: string | null;
  llm_model: string | null;
  duration_ms: number;
  error: string | null;
}

// Settings
export interface Settings {
  default_stt_provider: string;
  default_stt_model: string;
  default_llm_provider: string;
  default_llm_model: string;
  active_mode_key: string;
  input_device: string;
  auto_paste: boolean;
  context_awareness: boolean;
  language: string;
  whisper_server_url?: string;
}

// Recording status response
export interface RecordingStatusResponse {
  status: RecordingStatus;
  is_recording: boolean;
}

// Export format
export type ExportFormat = "txt" | "md" | "srt" | "vtt";

// History query
export interface HistoryQuery {
  limit?: number;
  offset?: number;
  search?: string;
}
