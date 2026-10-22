//! `OxideAV` adapter: local containers, packet framing and decoded pixels/PCM.
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]
use super::Info;
use oxideav_core::{
    AudioFrame, CodecOptions, Decoder, Demuxer, Error, Frame, MediaType, Packet, PixelFormat,
    RuntimeContext, SampleFormat, StreamInfo, VideoFrame,
};
use std::{collections::VecDeque, fs::File, path::Path};
type Result<T> = std::result::Result<T, String>;

pub(super) enum Decoded {
    Video {
        pts: f64,
        size: [usize; 2],
        rgba: Vec<u8>,
    },
    Audio {
        pts: f64,
        rate: u32,
        channels: usize,
        samples: Vec<f32>,
    },
}
impl Decoded {
    pub(super) fn pts(&self) -> f64 {
        match self {
            Self::Video { pts, .. } | Self::Audio { pts, .. } => *pts,
        }
    }
}
struct Track {
    info: StreamInfo,
    decoder: Box<dyn Decoder>,
    next_pts: f64,
}
pub(super) struct Source {
    demux: Box<dyn Demuxer>,
    tracks: Vec<Track>,
    ready: VecDeque<Decoded>,
    eof: bool,
}
fn registry() -> RuntimeContext {
    let mut ctx = RuntimeContext::new();
    oxideav_meta::register_all(&mut ctx);
    ctx
}
fn container(path: &Path, ctx: &RuntimeContext) -> Result<Box<dyn Demuxer>> {
    let mut file = File::open(path).map_err(|e| e.to_string())?;
    let format = ctx
        .containers
        .probe_input(&mut file, path.extension().and_then(|s| s.to_str()))
        .map_err(|e| e.to_string())?;
    ctx.containers
        .open_demuxer(&format, Box::new(file), &ctx.codecs)
        .map_err(|e| e.to_string())
}
pub(super) fn probe(path: &Path) -> Result<Info> {
    let ctx = registry();
    let demux = container(path, &ctx)?;
    let video = demux
        .streams()
        .iter()
        .find(|s| s.params.media_type == MediaType::Video);
    let audio = demux
        .streams()
        .iter()
        .any(|s| s.params.media_type == MediaType::Audio);
    if video.is_none() && !audio {
        return Err("No playable audio or video stream".into());
    }
    let size = video
        .map(|s| dimensions(s).map(|[w, h]| fit(w, h)))
        .transpose()?;
    let duration = demux
        .duration_micros()
        .map(|n| n as f64 / 1_000_000.0)
        .or_else(|| {
            demux
                .streams()
                .iter()
                .filter_map(|s| s.duration.map(|n| s.time_base.seconds_of(n)))
                .reduce(f64::max)
        })
        .unwrap_or(0.0)
        .max(0.0);
    Ok(Info {
        path: path.into(),
        duration,
        size,
        audio,
    })
}
fn dimensions(stream: &StreamInfo) -> Result<[u32; 2]> {
    let w = stream.params.width.unwrap_or(0);
    let h = stream.params.height.unwrap_or(0);
    if w == 0 || h == 0 || u64::from(w) * u64::from(h) > 33_177_600 {
        return Err("Video exceeds preview limit (33 megapixels) or has no dimensions".into());
    }
    Ok([w, h])
}
fn fit(w: u32, h: u32) -> [usize; 2] {
    let scale = (1280.0 / f64::from(w)).min(720.0 / f64::from(h)).min(1.0);
    [
        (f64::from(w) * scale).round().max(1.0) as usize,
        (f64::from(h) * scale).round().max(1.0) as usize,
    ]
}
impl Source {
    pub(super) fn open(path: &Path) -> Result<Self> {
        let ctx = registry();
        let mut demux = container(path, &ctx)?;
        let mut tracks = Vec::new();
        let mut video = false;
        let mut audio = false;
        for stream in demux.streams() {
            let selected = match stream.params.media_type {
                MediaType::Video if !video => {
                    video = true;
                    true
                }
                MediaType::Audio if !audio => {
                    audio = true;
                    true
                }
                _ => false,
            };
            if !selected {
                continue;
            }
            let mut info = stream.clone();
            if info.params.media_type == MediaType::Video {
                dimensions(&info)?;
            }
            // MP4 sample descriptions carry container metadata in the same bag
            // as codec options. Compressed codec configuration lives in extradata.
            if matches!(demux.format_name(), "mp4" | "mov") {
                if info.params.options.get("protection_scheme").is_some() {
                    return Err("Encrypted media is unsupported".into());
                }
                info.params.options = CodecOptions::new();
            }
            info.params.limits.max_pixels_per_frame = 33_177_600;
            info.params.limits.max_alloc_bytes_per_frame = 256 * 1024 * 1024;
            let decoder = ctx
                .codecs
                .first_decoder(&info.params)
                .map_err(|e| format!("OxideAV {}: {e}", info.params.codec_id))?;
            tracks.push(Track {
                info,
                decoder,
                next_pts: 0.0,
            });
        }
        if tracks.is_empty() {
            return Err("No supported media streams".into());
        }
        demux.set_active_streams(&tracks.iter().map(|t| t.info.index).collect::<Vec<_>>());
        Ok(Self {
            demux,
            tracks,
            ready: VecDeque::new(),
            eof: false,
        })
    }
    pub(super) fn seek(&mut self, seconds: f64) -> Result<()> {
        let track = self
            .tracks
            .iter()
            .find(|t| t.info.params.media_type == MediaType::Video)
            .unwrap_or(&self.tracks[0]);
        match self
            .demux
            .seek_to(track.info.index, track.info.time_base.ticks_of(seconds))
        {
            Ok(_) => {
                for track in &mut self.tracks {
                    // reset() currently drops H.264 SPS/PPS state in OxideAV.
                    // Reconstruct from the stream's configuration after a seek.
                    track.decoder = registry()
                        .codecs
                        .first_decoder(&track.info.params)
                        .map_err(|e| e.to_string())?;
                    track.next_pts = seconds;
                }
                self.ready.clear();
                self.eof = false;
                Ok(())
            }
            Err(Error::Unsupported(_)) => Ok(()), // Fresh source: decode forwards when no index exists.
            Err(e) => Err(e.to_string()),
        }
    }
    pub(super) fn next(&mut self) -> Result<Option<Decoded>> {
        loop {
            if let Some(frame) = self.ready.pop_front() {
                return Ok(Some(frame));
            }
            if self.eof {
                return Ok(None);
            }
            match self.demux.next_packet() {
                Ok(mut packet) => {
                    let Some(track) = self
                        .tracks
                        .iter_mut()
                        .find(|t| t.info.index == packet.stream_index)
                    else {
                        continue;
                    };
                    if packet.data.len() > 64 * 1024 * 1024 {
                        return Err("Media packet exceeds preview limit".into());
                    }
                    frame_aac(&track.info, &mut packet)?;
                    track
                        .decoder
                        .send_packet(&packet)
                        .map_err(|e| format!("OxideAV {}: {e}", track.info.params.codec_id))?;
                    drain(
                        track,
                        packet.pts.map(|n| packet.time_base.seconds_of(n)),
                        &mut self.ready,
                    )?;
                }
                Err(Error::Eof) => {
                    self.eof = true;
                    for track in &mut self.tracks {
                        track.decoder.flush().map_err(|e| e.to_string())?;
                        drain(track, None, &mut self.ready)?;
                    }
                }
                Err(e) => return Err(e.to_string()),
            }
        }
    }
}
fn drain(track: &mut Track, packet_pts: Option<f64>, ready: &mut VecDeque<Decoded>) -> Result<()> {
    loop {
        let frame = match track.decoder.receive_frame() {
            Ok(f) => f,
            Err(Error::NeedMore | Error::Eof) => return Ok(()),
            Err(e) => return Err(e.to_string()),
        };
        let pts = frame
            .pts()
            .map(|n| track.info.time_base.seconds_of(n))
            .or(packet_pts)
            .unwrap_or(track.next_pts);
        match frame {
            Frame::Video(frame) => {
                let [w, h] = dimensions(&track.info)?;
                let format = track
                    .decoder
                    .output_pixel_format()
                    .or(track.info.params.pixel_format)
                    .or_else(|| {
                        let p = frame.image_planes();
                        (p.len() == 3
                            && p[0].stride == w as usize
                            && p[1].stride == w.div_ceil(2) as usize
                            && p[1].data.len() == w.div_ceil(2) as usize * h.div_ceil(2) as usize)
                            .then_some(PixelFormat::Yuv420P)
                    })
                    .ok_or("OxideAV decoder did not report its pixel layout")?;
                let (size, rgba) = rgba(&frame, format, w, h, track.info.params.color_signal)?;
                ready.push_back(Decoded::Video { pts, size, rgba });
                track.next_pts = pts
                    + track
                        .info
                        .params
                        .frame_rate
                        .map_or(1.0 / 30.0, |r| r.den as f64 / r.num.max(1) as f64);
            }
            Frame::Audio(frame) => {
                let rate = track
                    .info
                    .params
                    .sample_rate
                    .ok_or("Audio sample rate missing")?;
                let channels = track.info.params.channels.ok_or("Audio channels missing")? as usize;
                if !(1..=2).contains(&channels) || !(8_000..=192_000).contains(&rate) {
                    return Err(
                        "OxideAV preview currently supports mono/stereo audio at 8-192 kHz".into(),
                    );
                }
                let format = track.info.params.sample_format.unwrap_or_else(|| {
                    match track.info.params.codec_id.as_str() {
                        "vorbis" => SampleFormat::F32P,
                        _ => SampleFormat::S16,
                    }
                });
                let samples = pcm(&frame, format, channels)?;
                track.next_pts = pts + f64::from(frame.samples) / f64::from(rate);
                ready.push_back(Decoded::Audio {
                    pts,
                    rate,
                    channels,
                    samples,
                });
            }
            _ => {}
        }
    }
}
fn rgba(
    frame: &VideoFrame,
    format: PixelFormat,
    w: u32,
    h: u32,
    signal: oxideav_core::ColorSignal,
) -> Result<([usize; 2], Vec<u8>)> {
    let frame = frame
        .clone()
        .with_color_signal(frame.color_signal().unwrap_or_default().or(signal));
    let converted = oxideav_pixfmt::convert(
        &frame,
        oxideav_pixfmt::FrameInfo::new(format, w, h),
        PixelFormat::Rgba,
        &oxideav_pixfmt::ConvertOptions::default(),
    )
    .map_err(|e| e.to_string())?;
    let plane = converted.image_planes().first().ok_or("No video pixels")?;
    let mut bytes = Vec::with_capacity(w as usize * h as usize * 4);
    for row in 0..h as usize {
        let start = row * plane.stride;
        bytes.extend_from_slice(
            plane
                .data
                .get(start..start + w as usize * 4)
                .ok_or("Truncated video plane")?,
        );
    }
    let size = fit(w, h);
    if size != [w as usize, h as usize] {
        let image = image::RgbaImage::from_raw(w, h, bytes).ok_or("Invalid RGBA frame")?;
        bytes = image::imageops::resize(
            &image,
            size[0] as u32,
            size[1] as u32,
            image::imageops::FilterType::Triangle,
        )
        .into_raw();
    }
    Ok((size, bytes))
}
fn pcm(frame: &AudioFrame, format: SampleFormat, channels: usize) -> Result<Vec<f32>> {
    let bytes = format.bytes_per_sample();
    let count = frame.samples as usize;
    if count > 192_000 * 10 {
        return Err("Audio packet exceeds preview limit".into());
    }
    let mut out = Vec::with_capacity(count * channels);
    for n in 0..count {
        for ch in 0..channels {
            let plane = if format.is_planar() { ch } else { 0 };
            let index = if format.is_planar() {
                n
            } else {
                n * channels + ch
            } * bytes;
            let b = frame
                .data
                .get(plane)
                .and_then(|p| p.get(index..index + bytes))
                .ok_or("Truncated PCM frame")?;
            let v = match format {
                SampleFormat::U8 | SampleFormat::U8P => (f32::from(b[0]) - 128.0) / 128.0,
                SampleFormat::S8 => f32::from(b[0].cast_signed()) / 128.0,
                SampleFormat::S16 | SampleFormat::S16P => {
                    f32::from(i16::from_le_bytes(b.try_into().unwrap())) / 32768.0
                }
                SampleFormat::S24 => {
                    (i32::from_le_bytes([0, b[0], b[1], b[2]]) as f32) / 2_147_483_648.0
                }
                SampleFormat::S32 | SampleFormat::S32P => {
                    i32::from_le_bytes(b.try_into().unwrap()) as f32 / 2_147_483_648.0
                }
                SampleFormat::F32 | SampleFormat::F32P => f32::from_le_bytes(b.try_into().unwrap()),
                SampleFormat::F64 | SampleFormat::F64P => {
                    f64::from_le_bytes(b.try_into().unwrap()) as f32
                }
                _ => return Err("Unsupported PCM sample layout".into()),
            };
            out.push(if v.is_finite() {
                v.clamp(-1.0, 1.0)
            } else {
                0.0
            });
        }
    }
    Ok(out)
}
fn frame_aac(stream: &StreamInfo, packet: &mut Packet) -> Result<()> {
    if stream.params.codec_id.as_str() != "aac"
        || packet.data.starts_with(&[0xff, 0xf1])
        || packet.data.starts_with(&[0xff, 0xf9])
    {
        return Ok(());
    }
    let c = &stream.params.extradata;
    if c.len() < 2 {
        return Err("Raw AAC requires AudioSpecificConfig".into());
    }
    let object = c[0] >> 3;
    let frequency = ((c[0] & 7) << 1) | (c[1] >> 7);
    let channels = (c[1] >> 3) & 15;
    if object != 2 || frequency > 12 || channels == 0 || channels > 7 {
        return Err("OxideAV raw AAC preview requires AAC-LC with an indexed sample rate".into());
    }
    let length = packet.data.len() + 7;
    if length > 8191 {
        return Err("AAC packet exceeds ADTS frame limit".into());
    }
    let mut framed = vec![
        0xff,
        0xf1,
        ((object - 1) << 6) | (frequency << 2) | (channels >> 2),
        ((channels & 3) << 6) | ((length >> 11) as u8),
        (length >> 3) as u8,
        ((length & 7) as u8) << 5 | 0x1f,
        0xfc,
    ];
    framed.extend_from_slice(&packet.data);
    packet.data = framed;
    Ok(())
}

#[cfg(test)]
pub(crate) fn silent_wav(path: &Path) {
    let data_size = 16000_u32 * 2 * 3;
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&(36 + data_size).to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16_u32.to_le_bytes());
    bytes.extend_from_slice(&1_u16.to_le_bytes());
    bytes.extend_from_slice(&1_u16.to_le_bytes());
    bytes.extend_from_slice(&16000_u32.to_le_bytes());
    bytes.extend_from_slice(&32000_u32.to_le_bytes());
    bytes.extend_from_slice(&2_u16.to_le_bytes());
    bytes.extend_from_slice(&16_u16.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&data_size.to_le_bytes());
    bytes.resize(44 + data_size as usize, 0);
    std::fs::write(path, bytes).unwrap();
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pcm_formats_and_invalid_planes() {
        let frame = AudioFrame {
            samples: 2,
            pts: None,
            data: vec![vec![0, 128, 255, 127]],
        };
        assert_eq!(
            pcm(&frame, SampleFormat::S16, 1).unwrap(),
            vec![-1.0, 32767.0 / 32768.0]
        );
        assert!(pcm(&frame, SampleFormat::S16, 2).is_err());
        let frame = AudioFrame {
            samples: 1,
            pts: None,
            data: vec![f32::NAN.to_le_bytes().to_vec()],
        };
        assert_eq!(pcm(&frame, SampleFormat::F32, 1).unwrap(), vec![0.0]);
    }
    #[test]
    fn wav_probe_seek_and_decode_without_helpers() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("silent.wav");
        silent_wav(&path);
        assert!((probe(&path).unwrap().duration - 3.0).abs() < 0.01);
        let mut source = Source::open(&path).unwrap();
        source.seek(1.0).unwrap();
        let Some(Decoded::Audio {
            pts,
            rate,
            channels,
            samples,
        }) = source.next().unwrap()
        else {
            panic!("expected audio");
        };
        assert!((pts - 1.0).abs() < 0.1);
        assert_eq!(rate, 16000);
        assert_eq!(channels, 1);
        assert!(samples.iter().all(|s| *s == 0.0));
    }
    #[test]
    fn representative_files_decode_real_pixels_and_pcm() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/media");
        for (name, video, audio) in [
            ("h264-aac.mp4", true, true),
            ("vp8.webm", true, false),
            ("vp9.webm", true, false),
            ("mpeg4.mp4", true, false),
            ("audio.mp3", false, true),
            ("audio.flac", false, true),
        ] {
            let path = root.join(name);
            let mut source = Source::open(&path).unwrap_or_else(|e| panic!("{name}: {e}"));
            let (mut frames, mut samples) = (0, 0);
            while let Some(frame) = source.next().unwrap_or_else(|e| panic!("{name}: {e}")) {
                match frame {
                    Decoded::Video { size, rgba, .. } => {
                        assert_eq!(rgba.len(), size[0] * size[1] * 4);
                        assert!(rgba.chunks_exact(4).any(|p| p[0] != p[1]));
                        frames += 1;
                    }
                    Decoded::Audio { samples: pcm, .. } => {
                        samples += pcm.len();
                    }
                }
            }
            assert_eq!(frames, if video { 10 } else { 0 }, "{name}");
            assert_eq!(samples > 0, audio, "{name}");
        }
    }
}
