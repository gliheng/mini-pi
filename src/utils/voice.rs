use std::io::Cursor;
use std::sync::{Arc, Mutex};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

/// Current state of the voice-input button.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum VoiceState {
    Idle,
    Recording,
    Transcribing,
}

/// Holds the live audio stream and the accumulating sample buffer.
pub struct VoiceRecorder {
    _stream: cpal::Stream,
    samples: Arc<Mutex<Vec<f32>>>,
    sample_rate: u32,
    channels: u16,
}

/// Start capturing audio from the default microphone.
pub fn start_recording() -> Result<VoiceRecorder, String> {
    let host = cpal::default_host();
    let device = host
        .default_input_device()
        .ok_or_else(|| "no microphone found".to_string())?;
    let config = device
        .default_input_config()
        .map_err(|e| format!("failed to read mic config: {}", e))?;

    let sample_rate = config.sample_rate().0;
    let channels = config.channels();
    let format = config.sample_format();

    let samples = Arc::new(Mutex::new(Vec::<f32>::new()));
    let samples_for_callback = samples.clone();

    let err_fn = |err| eprintln!("[voice] stream error: {}", err);

    let stream = match format {
        cpal::SampleFormat::F32 => device.build_input_stream(
            &config.into(),
            move |data: &[f32], _: &_| {
                samples_for_callback.lock().unwrap().extend_from_slice(data);
            },
            err_fn,
            None,
        ),
        cpal::SampleFormat::I16 => device.build_input_stream(
            &config.into(),
            move |data: &[i16], _: &_| {
                samples_for_callback
                    .lock()
                    .unwrap()
                    .extend(data.iter().map(|s| *s as f32 / i16::MAX as f32));
            },
            err_fn,
            None,
        ),
        cpal::SampleFormat::U16 => device.build_input_stream(
            &config.into(),
            move |data: &[u16], _: &_| {
                samples_for_callback.lock().unwrap().extend(data.iter().map(|s| {
                    (*s as f32 / u16::MAX as f32).mul_add(2.0, -1.0)
                }));
            },
            err_fn,
            None,
        ),
        other => {
            return Err(format!("unsupported microphone sample format: {:?}", other));
        }
    }
    .map_err(|e| format!("failed to open microphone stream: {}", e))?;

    stream
        .play()
        .map_err(|e| format!("failed to start microphone: {}", e))?;

    Ok(VoiceRecorder {
        _stream: stream,
        samples,
        sample_rate,
        channels,
    })
}

impl VoiceRecorder {
    /// Stop the stream and return a 16 kHz mono WAV file as bytes.
    pub fn stop(self) -> Vec<u8> {
        drop(self._stream);

        let samples = Arc::try_unwrap(self.samples)
            .map(|m| m.into_inner().unwrap())
            .unwrap_or_else(|s| s.lock().unwrap().clone());

        // Convert to mono.
        let mono: Vec<f32> = if self.channels == 1 {
            samples
        } else {
            samples
                .chunks(self.channels.max(1) as usize)
                .map(|chunk| chunk.iter().sum::<f32>() / chunk.len() as f32)
                .collect()
        };

        // Resample to 16 kHz using linear interpolation.
        let target_rate = 16_000_f32;
        let source_rate = self.sample_rate as f32;
        let ratio = source_rate / target_rate;
        let out_len = (mono.len() as f32 / ratio) as usize;
        let mut resampled = Vec::with_capacity(out_len);
        for i in 0..out_len {
            let src_idx = i as f32 * ratio;
            let i0 = src_idx.floor() as usize;
            let i1 = (i0 + 1).min(mono.len().saturating_sub(1));
            let frac = src_idx - i0 as f32;
            let s0 = mono.get(i0).copied().unwrap_or(0.0);
            let s1 = mono.get(i1).copied().unwrap_or(0.0);
            resampled.push(s0 + (s1 - s0) * frac);
        }

        // Convert f32 [-1, 1] to 16-bit PCM.
        let pcm: Vec<i16> = resampled
            .iter()
            .map(|s| (s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16)
            .collect();

        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: 16_000,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };

        let mut cursor = Cursor::new(Vec::new());
        {
            let mut writer = hound::WavWriter::new(&mut cursor, spec)
                .expect("WavWriter should write to in-memory cursor");
            for sample in pcm {
                writer.write_sample(sample).unwrap();
            }
            writer.finalize().unwrap();
        }
        cursor.into_inner()
    }
}

/// Worker host for voice transcription.
const TRANSCRIBE_WORKER_URL: &str = "https://pi.raven-ai.one";

/// Send the WAV audio to the pi-commander worker `/api/transcribe` endpoint and return the transcript.
///
/// The public API is async so callers can `.await` it, but the actual HTTP request runs on
/// `smol`'s blocking thread pool because `reqwest`'s async client requires a Tokio runtime,
/// while this app runs on `smol`.
pub async fn transcribe(wav_bytes: &[u8]) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    let wav_bytes = wav_bytes.to_vec();
    smol::unblock(move || transcribe_sync(&wav_bytes)).await
}

fn transcribe_sync(wav_bytes: &[u8]) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    let client = reqwest::blocking::Client::new();
    let url = format!("{}/api/transcribe", TRANSCRIBE_WORKER_URL.trim_end_matches('/'));

    let audio_data_uri = format!(
        "data:audio/wav;base64,{}",
        base64::Engine::encode(&base64::engine::general_purpose::STANDARD, wav_bytes)
    );

    let body = serde_json::json!({
        "dataUrl": audio_data_uri
    });

    let response = client
        .post(&url)
        .header("Content-Type", "application/json")
        .json(&body)
        .send()?;

    let status = response.status();
    if !status.is_success() {
        let body = response.text().unwrap_or_default();
        return Err(format!("transcribe worker returned {}: {}", status, body).into());
    }

    let json: serde_json::Value = response.json()?;
    let text = json
        .get("text")
        .and_then(|c| c.as_str())
        .ok_or_else(|| {
            format!(
                "unexpected transcribe response format: {}",
                serde_json::to_string(&json).unwrap_or_default()
            )
        })?;

    Ok(text.trim().to_string())
}
