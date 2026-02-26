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
    /// Current audio level (RMS, 0.0–1.0) stored as f32 bits in an AtomicU32
    /// so the hot audio callback never needs a lock.
    current_level: Arc<AtomicU32>,
    /// Peak level, same encoding as current_level.
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
        self.is_recording.load(Ordering::SeqCst)
    }

    pub fn set_recording(&self, recording: bool) {
        self.is_recording.store(recording, Ordering::SeqCst);
    }

    pub fn clear_samples(&self) {
        if let Ok(mut samples) = self.samples.lock() {
            samples.clear();
        }
    }

    pub fn get_samples(&self) -> Vec<f32> {
        self.samples.lock().map(|s| s.clone()).unwrap_or_default()
    }

    pub fn append_samples(&self, new_samples: Vec<f32>) {
        if let Ok(mut samples) = self.samples.lock() {
            samples.extend(new_samples);
        }
    }

    /// Update audio level from new samples (lock-free).
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
        let peak = new_samples.iter().map(|s| s.abs()).fold(0.0f32, f32::max);

        // Relaxed ordering is fine: readers only need an approximately current value.
        self.current_level.store(level.to_bits(), Ordering::Relaxed);
        self.peak_level.store(peak.min(1.0).to_bits(), Ordering::Relaxed);
    }

    /// Get current audio level (lock-free).
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
pub type LevelCallback = Box<dyn Fn(f32) + Send + 'static>;

/// Start recording in a separate thread (returns immediately)
/// The stream is managed in the spawned thread
/// Optional level_callback is called with audio level (0.0-1.0) periodically
pub fn start_recording(
    handle: RecordingHandle,
    device_name: &str,
    level_callback: Option<LevelCallback>,
) -> Result<()> {
    if handle.is_recording() {
        return Err(AppError::RecordingInProgress);
    }

    let device = get_device_by_name(device_name)?;
    let config = device.default_input_config()?;

    log::info!(
        "Starting recording on device: {} (format: {:?}, rate: {}, channels: {})",
        device.name().unwrap_or_default(),
        config.sample_format(),
        config.sample_rate().0,
        config.channels()
    );

    handle.clear_samples();
    handle.set_recording(true);

    let source_sample_rate = config.sample_rate().0;
    let channels = config.channels() as usize;
    let sample_format = config.sample_format();
    let handle_clone = handle.clone();

    // Spawn a thread to manage the stream (Stream is not Send)
    std::thread::spawn(move || {
        let err_fn = |err| {
            log::error!("Audio stream error: {}", err);
        };

        let stream_config: StreamConfig = config.into();

        let samples_ref = handle_clone.samples.clone();
        let is_recording_ref = handle_clone.is_recording.clone();
        let level_handle = handle_clone.clone();
        let level_handle2 = handle_clone.clone();
        let level_handle3 = handle_clone.clone();

        let stream_result = match sample_format {
            SampleFormat::F32 => device.build_input_stream(
                &stream_config,
                move |data: &[f32], _: &_| {
                    if is_recording_ref.load(Ordering::SeqCst) {
                        let processed = process_audio_data(data, source_sample_rate, channels);
                        level_handle.update_level(&processed);
                        if let Ok(mut samples) = samples_ref.lock() {
                            samples.extend(processed);
                        }
                    }
                },
                err_fn,
                None,
            ),
            SampleFormat::I16 => {
                let samples_ref = handle_clone.samples.clone();
                let is_recording_ref = handle_clone.is_recording.clone();
                device.build_input_stream(
                    &stream_config,
                    move |data: &[i16], _: &_| {
                        if is_recording_ref.load(Ordering::SeqCst) {
                            let float_data: Vec<f32> =
                                data.iter().map(|&s| s as f32 / i16::MAX as f32).collect();
                            let processed = process_audio_data(&float_data, source_sample_rate, channels);
                            level_handle2.update_level(&processed);
                            if let Ok(mut samples) = samples_ref.lock() {
                                samples.extend(processed);
                            }
                        }
                    },
                    err_fn,
                    None,
                )
            }
            SampleFormat::U16 => {
                let samples_ref = handle_clone.samples.clone();
                let is_recording_ref = handle_clone.is_recording.clone();
                device.build_input_stream(
                    &stream_config,
                    move |data: &[u16], _: &_| {
                        if is_recording_ref.load(Ordering::SeqCst) {
                            let float_data: Vec<f32> = data
                                .iter()
                                .map(|&s| (s as f32 / u16::MAX as f32) * 2.0 - 1.0)
                                .collect();
                            let processed = process_audio_data(&float_data, source_sample_rate, channels);
                            level_handle3.update_level(&processed);
                            if let Ok(mut samples) = samples_ref.lock() {
                                samples.extend(processed);
                            }
                        }
                    },
                    err_fn,
                    None,
                )
            }
            _ => {
                log::error!("Unsupported sample format: {:?}", sample_format);
                return;
            }
        };

        match stream_result {
            Ok(stream) => {
                if let Err(e) = stream.play() {
                    log::error!("Failed to play stream: {}", e);
                    handle_clone.set_recording(false);
                    return;
                }

                // Keep the thread alive while recording
                // Also emit level updates via callback
                let mut last_level_update = std::time::Instant::now();
                while handle_clone.is_recording() {
                    std::thread::sleep(std::time::Duration::from_millis(30));

                    // Emit level callback every ~100ms
                    if last_level_update.elapsed() >= std::time::Duration::from_millis(100) {
                        if let Some(ref cb) = level_callback {
                            let (level, _peak) = handle_clone.get_level();
                            cb(level);
                        }
                        last_level_update = std::time::Instant::now();
                    }
                }

                // Stream will be dropped here, stopping the recording
                log::info!("Recording thread finished");
            }
            Err(e) => {
                log::error!("Failed to build stream: {}", e);
                handle_clone.set_recording(false);
            }
        }
    });

    Ok(())
}

/// Stop recording and return samples
pub fn stop_recording(handle: &RecordingHandle) -> Result<Vec<f32>> {
    if !handle.is_recording() {
        return Err(AppError::NoRecordingInProgress);
    }

    handle.set_recording(false);

    // Give the recording thread time to finish
    std::thread::sleep(std::time::Duration::from_millis(100));

    let samples = handle.get_samples();
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
    resample(&mono, source_rate, WHISPER_SAMPLE_RATE)
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

/// Save audio samples to a WAV file
pub fn save_wav(samples: &[f32], path: &PathBuf) -> Result<()> {
    let spec = WavSpec {
        channels: 1,
        sample_rate: WHISPER_SAMPLE_RATE,
        bits_per_sample: 16,
        sample_format: HoundSampleFormat::Int,
    };

    let mut writer = WavWriter::create(path, spec)?;

    for &sample in samples {
        // Convert f32 [-1.0, 1.0] to i16
        let sample_i16 = (sample.clamp(-1.0, 1.0) * i16::MAX as f32) as i16;
        writer.write_sample(sample_i16)?;
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
        resample(&samples, spec.sample_rate, WHISPER_SAMPLE_RATE)
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
