//! CPAL device output. Only bounded interleaved PCM crosses into the callback.
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::{
    collections::VecDeque,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};
pub(super) struct Sink {
    _stream: cpal::Stream,
    queue: Arc<Mutex<VecDeque<f32>>>,
    error: Arc<Mutex<Option<String>>>,
    rate: u32,
    channels: usize,
    input: Vec<f32>,
    phase: f64,
    source_rate: u32,
    source_channels: usize,
}
impl Sink {
    pub(super) fn open(muted: Arc<AtomicBool>) -> Result<Self, String> {
        let device = cpal::default_host()
            .default_output_device()
            .ok_or("No audio output device")?;
        let supported = device.default_output_config().map_err(|e| e.to_string())?;
        let format = supported.sample_format();
        let config: cpal::StreamConfig = supported.into();
        let rate = config.sample_rate;
        let channels = config.channels as usize;
        let queue = Arc::new(Mutex::new(VecDeque::new()));
        let error = Arc::new(Mutex::new(None));
        let stream = match format {
            cpal::SampleFormat::F32 => {
                build::<f32>(&device, config, queue.clone(), error.clone(), muted)
            }
            cpal::SampleFormat::I16 => {
                build::<i16>(&device, config, queue.clone(), error.clone(), muted)
            }
            cpal::SampleFormat::U16 => {
                build::<u16>(&device, config, queue.clone(), error.clone(), muted)
            }
            cpal::SampleFormat::I32 => {
                build::<i32>(&device, config, queue.clone(), error.clone(), muted)
            }
            cpal::SampleFormat::F64 => {
                build::<f64>(&device, config, queue.clone(), error.clone(), muted)
            }
            _ => return Err(format!("Unsupported audio output format: {format:?}")),
        }?;
        stream.play().map_err(|e| e.to_string())?;
        Ok(Self {
            _stream: stream,
            queue,
            error,
            rate,
            channels,
            input: Vec::new(),
            phase: 0.0,
            source_rate: 0,
            source_channels: 0,
        })
    }
    pub(super) fn check(&self) -> Result<(), String> {
        self.error
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take()
            .map_or(Ok(()), Err)
    }
    pub(super) fn feed(
        &mut self,
        samples: &[f32],
        rate: u32,
        channels: usize,
        cancel: &AtomicBool,
    ) -> Result<(), String> {
        if (rate, channels) != (self.source_rate, self.source_channels) {
            self.input.clear();
            self.phase = 0.0;
            self.source_rate = rate;
            self.source_channels = channels;
        }
        self.input.extend_from_slice(samples);
        let frames = self.input.len() / channels;
        let ratio = f64::from(rate) / f64::from(self.rate);
        let mut output = Vec::new();
        while self.phase + 1.0 < frames as f64 {
            let at = self.phase as usize;
            let frac = (self.phase - at as f64) as f32;
            for channel in 0..self.channels {
                let sample = |index: usize| {
                    if self.channels == 1 && channels == 2 {
                        (self.input[index * channels] + self.input[index * channels + 1]) * 0.5
                    } else if channel >= 2 {
                        0.0 // Keep unrelated surround/LFE device channels silent.
                    } else {
                        self.input[index * channels + channel.min(channels - 1)]
                    }
                };
                output.push(sample(at) * (1.0 - frac) + sample(at + 1) * frac);
            }
            self.phase += ratio;
        }
        let consumed = (self.phase as usize).min(frames.saturating_sub(1));
        self.input.drain(..consumed * channels);
        self.phase -= consumed as f64;
        let capacity = (self.rate as usize * self.channels) / 2;
        let mut offset = 0;
        while offset < output.len() && !cancel.load(Ordering::Relaxed) {
            self.check()?;
            {
                let mut queue = self
                    .queue
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                let n = (capacity.saturating_sub(queue.len())).min(output.len() - offset);
                queue.extend(&output[offset..offset + n]);
                offset += n;
            }
            if offset < output.len() {
                std::thread::sleep(Duration::from_millis(5));
            }
        }
        Ok(())
    }
    pub(super) fn pending(&self) -> bool {
        !self
            .queue
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .is_empty()
    }
}
fn build<T: cpal::SizedSample + cpal::FromSample<f32>>(
    device: &cpal::Device,
    config: cpal::StreamConfig,
    queue: Arc<Mutex<VecDeque<f32>>>,
    error: Arc<Mutex<Option<String>>>,
    muted: Arc<AtomicBool>,
) -> Result<cpal::Stream, String> {
    device
        .build_output_stream(
            config,
            move |data: &mut [T], _: &cpal::OutputCallbackInfo| {
                let mut queue = queue
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                let mute = muted.load(Ordering::Relaxed);
                for sample in data {
                    let value = queue.pop_front().unwrap_or(0.0);
                    *sample = T::from_sample(if mute { 0.0 } else { value });
                }
            },
            move |failure| {
                *error
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(failure.to_string());
            },
            None,
        )
        .map_err(|e| e.to_string())
}
