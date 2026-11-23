//! Audio recording module for WhisperTray
//!
//! Handles microphone capture using cpal (which supports PipeWire, PulseAudio, ALSA)

use crate::error::{AppError, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{Device, SampleFormat, StreamConfig};
use hound::{SampleFormat as HoundSampleFormat, WavSpec, WavWriter};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

/// Audio sample rate for whisper.cpp (16kHz required)
pub const WHISPER_SAMPLE_RATE: u32 = 16000;

/// Audio input device information
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AudioDevice {
    pub name: String,
    pub is_default: bool,
}

/// Get list of available input devices
pub fn get_input_devices() -> Result<Vec<AudioDevice>> {
    let host = cpal::default_host();
    let default_device = host.default_input_device();
    let default_name = default_device
        .as_ref()
        .and_then(|d| d.name().ok())
        .unwrap_or_default();

    let devices = host.input_devices()?;
    let mut result = Vec::new();

    for device in devices {
        if let Ok(name) = device.name() {
            result.push(AudioDevice {
                is_default: name == default_name,
                name,
            });
        }
    }

    Ok(result)
}

/// Get a specific input device by name
pub fn get_device_by_name(name: &str) -> Result<Device> {
    let host = cpal::default_host();

    if name.is_empty() || name == "default" {
        return host
            .default_input_device()
            .ok_or_else(|| AppError::Audio("No default input device".to_string()));
    }

    host.input_devices()?
        .find(|d| d.name().map_or(false, |n| n == name))
        .ok_or_else(|| AppError::Audio(format!("Device not found: {}", name)))
}

/// Shared recording state (Send + Sync safe)
#[derive(Clone)]
pub struct RecordingHandle {
    /// Audio samples buffer (f32 normalized)
    samples: Arc<Mutex<Vec<f32>>>,
    /// Recording flag
    is_recording: Arc<AtomicBool>,
    /// Current audio level (RMS, 0.0 to 1.0) — stored as f32 bits for lock-free access
    current_level: Arc<AtomicU32>,
    /// Peak level — stored as f32 bits for lock-free access
    peak_level: Arc<AtomicU32>,
}

impl RecordingHandle {
    pub fn new() -> Self {
        Self {
            samples: Arc::new(Mutex::new(Vec::new())),
            is_recording: Arc::new(AtomicBool::new(false)),
            current_level: Arc::new(AtomicU32::new(0)),
            peak_level: Arc::new(AtomicU32::new(0)),
        }
    }

    pub fn is_recording(&self) -> bool {
        self.is_recording.load(Ordering::Acquire)
    }

    fn capture(&self, process: impl FnOnce() -> Vec<f32>) {
        if let Ok(mut samples) = self.samples.lock() {
            if self.is_recording() {
                let processed = process();
                self.update_level(&processed);
                samples.extend(processed);
            }
        }
    }

    fn finish(&self) -> Result<Vec<f32>> {
        let mut samples = self
            .samples
            .lock()
            .map_err(|_| AppError::Audio("Recording buffer is unavailable".to_string()))?;
        if !self.is_recording.swap(false, Ordering::AcqRel) {
            return Err(AppError::NoRecordingInProgress);
        }
        Ok(std::mem::take(&mut *samples))
    }

    /// Update audio level from new samples (lock-free, safe to call from audio callback)
    pub fn update_level(&self, new_samples: &[f32]) {
        if new_samples.is_empty() {
            return;
        }

        // Calculate RMS level
        let sum_sq: f32 = new_samples.iter().map(|s| s * s).sum();
        let rms = (sum_sq / new_samples.len() as f32).sqrt();

        // Scale to 0-1 range (typical speech is around 0.1-0.3 RMS)
        let level = (rms * 3.0).min(1.0);

        // Find peak
        let peak = new_samples
            .iter()
            .map(|s| s.abs())
            .fold(0.0f32, f32::max)
            .min(1.0);

        // Store as bit-pattern in AtomicU32 for lock-free access
        self.current_level.store(level.to_bits(), Ordering::Relaxed);
        self.peak_level.store(peak.to_bits(), Ordering::Relaxed);
    }

    /// Get current audio level (lock-free)
    pub fn get_level(&self) -> (f32, f32) {
        let level = f32::from_bits(self.current_level.load(Ordering::Relaxed));
        let peak = f32::from_bits(self.peak_level.load(Ordering::Relaxed));
        (level, peak)
    }
}

impl Default for RecordingHandle {
    fn default() -> Self {
        Self::new()
    }
}

/// Callback type for audio level updates
pub type LevelCallback = Box<dyn Fn(f32, f32) + Send + 'static>;

/// Start recording in a separate thread (returns immediately)
/// The stream is managed in the spawned thread
/// Optional level_callback is called with audio level (0.0-1.0) periodically
pub fn start_recording(
    device_name: &str,
    level_callback: Option<LevelCallback>,
) -> Result<RecordingHandle> {
    let device = get_device_by_name(device_name)?;
    let config = device.default_input_config()?;

    log::info!(
        "Starting recording on device: {} (format: {:?}, rate: {}, channels: {})",
        device.name().unwrap_or_default(),
        config.sample_format(),
        config.sample_rate().0,
        config.channels()
    );

    let source_sample_rate = config.sample_rate().0;
    let channels = config.channels() as usize;
    let sample_format = config.sample_format();
    if !matches!(
        sample_format,
        SampleFormat::F32 | SampleFormat::I16 | SampleFormat::U16
    ) {
        return Err(AppError::Audio(format!(
            "Unsupported sample format: {sample_format:?}"
        )));
    }

    // A fresh handle prevents a previous stream from writing into a new recording.
    let handle = RecordingHandle::new();
    handle.is_recording.store(true, Ordering::Release);
    let handle_clone = handle.clone();

    // Spawn a thread to manage the stream (Stream is not Send)
    std::thread::spawn(move || {
        let err_fn = |err| {
            log::error!("Audio stream error: {}", err);
        };

        let stream_config: StreamConfig = config.into();

        let capture = handle_clone.clone();

        let stream_result = match sample_format {
            SampleFormat::F32 => device.build_input_stream(
                &stream_config,
                move |data: &[f32], _: &_| {
                    capture.capture(|| process_audio_data(data, source_sample_rate, channels));
                },
                err_fn,
                None,
            ),
            SampleFormat::I16 => device.build_input_stream(
                &stream_config,
                move |data: &[i16], _: &_| {
                    capture.capture(|| process_audio_data_i16(data, source_sample_rate, channels));
                },
                err_fn,
                None,
            ),
            SampleFormat::U16 => device.build_input_stream(
                &stream_config,
                move |data: &[u16], _: &_| {
                    capture.capture(|| process_audio_data_u16(data, source_sample_rate, channels));
                },
                err_fn,
                None,
            ),
            _ => unreachable!("sample format already checked"),
        };

        match stream_result {
            Ok(stream) => {
                if let Err(e) = stream.play() {
                    log::error!("Failed to play stream: {}", e);
                    let _ = handle_clone.finish();
                    return;
                }

                // Keep the thread alive while recording.
                // Also emit level/peak updates via callback.
                let mut last_level_update = std::time::Instant::now();
                while handle_clone.is_recording() {
                    std::thread::sleep(std::time::Duration::from_millis(30));

                    // Emit callbacks at ~12.5 Hz to keep UI responsive with low overhead.
                    if handle_clone.is_recording()
                        && last_level_update.elapsed() >= std::time::Duration::from_millis(80)
                    {
                        if let Some(ref cb) = level_callback {
                            let (level, peak) = handle_clone.get_level();
                            cb(level, peak);
                        }
                        last_level_update = std::time::Instant::now();
                    }
                }

                // Stream will be dropped here, stopping the recording
                log::info!("Recording thread finished");
            }
            Err(e) => {
                log::error!("Failed to build stream: {}", e);
                let _ = handle_clone.finish();
            }
        }
    });

    Ok(handle)
}

/// Stop recording and return samples
pub fn stop_recording(handle: &RecordingHandle) -> Result<Vec<f32>> {
    let samples = handle.finish()?;
    log::info!("Recording stopped. {} samples captured", samples.len());

    Ok(samples)
}

/// Process incoming audio data: convert to mono and resample to 16kHz
fn process_audio_data(data: &[f32], source_rate: u32, channels: usize) -> Vec<f32> {
    // Convert to mono by averaging channels
    let mono: Vec<f32> = data
        .chunks(channels)
        .map(|chunk| chunk.iter().sum::<f32>() / channels as f32)
        .collect();

    // Simple linear resampling to 16kHz
    resample_owned(mono, source_rate, WHISPER_SAMPLE_RATE)
}

fn process_audio_data_i16(data: &[i16], source_rate: u32, channels: usize) -> Vec<f32> {
    let scale = 1.0 / i16::MAX as f32;
    let mono: Vec<f32> = data
        .chunks(channels)
        .map(|chunk| chunk.iter().map(|&s| s as f32 * scale).sum::<f32>() / channels as f32)
        .collect();

    resample_owned(mono, source_rate, WHISPER_SAMPLE_RATE)
}

fn process_audio_data_u16(data: &[u16], source_rate: u32, channels: usize) -> Vec<f32> {
    let scale = 2.0 / u16::MAX as f32;
    let mono: Vec<f32> = data
        .chunks(channels)
        .map(|chunk| chunk.iter().map(|&s| s as f32 * scale - 1.0).sum::<f32>() / channels as f32)
        .collect();

    resample_owned(mono, source_rate, WHISPER_SAMPLE_RATE)
}

/// Simple linear interpolation resampling
fn resample(samples: &[f32], from_rate: u32, to_rate: u32) -> Vec<f32> {
    if from_rate == to_rate {
        return samples.to_vec();
    }

    let ratio = from_rate as f64 / to_rate as f64;
    let new_len = (samples.len() as f64 / ratio) as usize;
    let mut resampled = Vec::with_capacity(new_len);

    for i in 0..new_len {
        let src_idx = i as f64 * ratio;
        let idx_floor = src_idx.floor() as usize;
        let idx_ceil = (idx_floor + 1).min(samples.len().saturating_sub(1));
        let frac = src_idx - idx_floor as f64;

        if idx_floor < samples.len() {
            let sample = samples[idx_floor] * (1.0 - frac as f32)
                + samples.get(idx_ceil).copied().unwrap_or(0.0) * frac as f32;
            resampled.push(sample);
        }
    }

    resampled
}

fn resample_owned(samples: Vec<f32>, from_rate: u32, to_rate: u32) -> Vec<f32> {
    if from_rate == to_rate {
        samples
    } else {
        resample(&samples, from_rate, to_rate)
    }
}

/// Save audio samples to a WAV file
pub fn save_wav(samples: &[f32], path: &PathBuf) -> Result<()> {
    let spec = WavSpec {
        channels: 1,
        sample_rate: WHISPER_SAMPLE_RATE,
        bits_per_sample: 16,
        sample_format: HoundSampleFormat::Int,
    };

    let mut writer = WavWriter::create(path, spec)?;

    for chunk in samples.chunks(WHISPER_SAMPLE_RATE as usize) {
        let mut pcm = writer.get_i16_writer(chunk.len() as u32);
        for &sample in chunk {
            pcm.write_sample((sample.clamp(-1.0, 1.0) * i16::MAX as f32) as i16);
        }
        pcm.flush()?;
    }

    writer.finalize()?;

    log::info!("Saved WAV file: {:?}", path);
    Ok(())
}

/// Load audio samples from a WAV file (for reprocessing)
pub fn load_wav(path: &PathBuf) -> Result<Vec<f32>> {
    let mut reader = hound::WavReader::open(path)?;
    let spec = reader.spec();

    let samples: Vec<f32> = match spec.sample_format {
        HoundSampleFormat::Float => reader.samples::<f32>().filter_map(|s| s.ok()).collect(),
        HoundSampleFormat::Int => {
            let max_val = (1 << (spec.bits_per_sample - 1)) as f32;
            reader
                .samples::<i32>()
                .filter_map(|s| s.ok())
                .map(|s| s as f32 / max_val)
                .collect()
        }
    };

    // Resample if necessary
    let samples = if spec.sample_rate != WHISPER_SAMPLE_RATE {
        resample_owned(samples, spec.sample_rate, WHISPER_SAMPLE_RATE)
    } else {
        samples
    };

    // Convert to mono if necessary
    let samples = if spec.channels > 1 {
        samples
            .chunks(spec.channels as usize)
            .map(|chunk| chunk.iter().sum::<f32>() / spec.channels as f32)
            .collect()
    } else {
        samples
    };

    Ok(samples)
}

/// Calculate audio duration in milliseconds
pub fn calculate_duration_ms(sample_count: usize) -> u64 {
    (sample_count as u64 * 1000) / WHISPER_SAMPLE_RATE as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stopping_takes_samples_and_late_callbacks_cannot_append() {
        let handle = RecordingHandle::new();
        handle.is_recording.store(true, Ordering::Release);
        let callback = handle.clone();
        callback.capture(|| vec![0.25, -0.5]);

        assert_eq!(stop_recording(&handle).unwrap(), vec![0.25, -0.5]);
        callback.capture(|| panic!("a stopped recording must reject late frames"));
        assert!(matches!(
            stop_recording(&handle),
            Err(AppError::NoRecordingInProgress)
        ));
    }

    #[test]
    fn wav_writer_flushes_a_partial_chunk() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("clip.wav");
        let mut samples = vec![0.25; WHISPER_SAMPLE_RATE as usize];
        samples.push(-0.5);
        save_wav(&samples, &path).unwrap();
        let decoded = load_wav(&path).unwrap();
        assert_eq!(decoded.len(), samples.len());
        for (actual, expected) in decoded.iter().zip(samples) {
            assert!((actual - expected).abs() <= 2.0 / i16::MAX as f32);
        }
    }

    #[test]
    fn test_resample_same_rate() {
        let samples = vec![0.0, 0.5, 1.0, 0.5, 0.0];
        let resampled = resample(&samples, 16000, 16000);
        assert_eq!(samples.len(), resampled.len());
    }

    #[test]
    fn test_resample_downsample() {
        let samples: Vec<f32> = (0..1000).map(|i| (i as f32 / 1000.0).sin()).collect();
        let resampled = resample(&samples, 48000, 16000);
        // Should be roughly 1/3 the size
        assert!(resampled.len() < samples.len());
        assert!(resampled.len() > samples.len() / 4);
    }

    #[test]
    fn test_calculate_duration() {
        // 16000 samples at 16kHz = 1 second = 1000 ms
        assert_eq!(calculate_duration_ms(16000), 1000);
        // 8000 samples = 500 ms
        assert_eq!(calculate_duration_ms(8000), 500);
    }
}
