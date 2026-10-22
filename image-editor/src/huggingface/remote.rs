//! Bounded range requests keep cancellation responsive during multi-gigabyte imports.
use super::{client, invalid, send, AtomicBool, EditError, File, Ordering, Read};
use std::io;
const CHUNK: u64 = 8 * 1024 * 1024;
pub(super) enum Remote<'a> {
    Small(reqwest::blocking::Response),
    Ranged(Ranged<'a>),
}
pub(super) struct Ranged<'a> {
    client: reqwest::blocking::Client,
    url: reqwest::Url,
    token: String,
    cancel: &'a AtomicBool,
    total: u64,
    position: u64,
    left: u64,
    response: Option<reqwest::blocking::Response>,
}
pub(super) fn reader<'a>(
    url: reqwest::Url,
    file: &File,
    token: &str,
    cancel: &'a AtomicBool,
) -> Result<Remote<'a>, EditError> {
    if cancel.load(Ordering::Acquire) {
        return Err(invalid("Download cancelled"));
    }
    let total = file.lfs.as_ref().map(|l| l.size).or(file.size).unwrap_or(0);
    if total <= CHUNK {
        return send(client(true)?.get(url), token).map(Remote::Small);
    }
    if total > 32 * 1024 * 1024 * 1024 {
        return Err(invalid("File exceeds 32 GiB limit"));
    }
    let mut reader = Ranged {
        client: client(true)?,
        url,
        token: token.into(),
        cancel,
        total,
        position: 0,
        left: 0,
        response: None,
    };
    reader.begin()?;
    Ok(Remote::Ranged(reader))
}
impl Ranged<'_> {
    fn begin(&mut self) -> Result<(), EditError> {
        let end = (self.position + CHUNK).min(self.total) - 1;
        let response = send(
            self.client.get(self.url.clone()).header(
                reqwest::header::RANGE,
                format!("bytes={}-{end}", self.position),
            ),
            &self.token,
        )?;
        let expected = format!("bytes {}-{end}/{}", self.position, self.total);
        if response.status() != reqwest::StatusCode::PARTIAL_CONTENT
            || response
                .headers()
                .get(reqwest::header::CONTENT_RANGE)
                .and_then(|h| h.to_str().ok())
                != Some(expected.as_str())
        {
            return Err(invalid(
                "Hugging Face did not return the requested file range; retry the download",
            ));
        }
        self.left = end - self.position + 1;
        self.response = Some(response);
        Ok(())
    }
}
impl Read for Remote<'_> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        match self {
            Self::Small(response) => response.read(buffer),
            Self::Ranged(reader) => {
                if buffer.is_empty() || reader.position == reader.total {
                    return Ok(0);
                }
                if reader.cancel.load(Ordering::Acquire) {
                    return Err(io::Error::new(
                        io::ErrorKind::Interrupted,
                        "Download cancelled",
                    ));
                }
                if reader.left == 0 {
                    reader
                        .begin()
                        .map_err(|e| io::Error::other(e.to_string()))?;
                }
                let len = buffer
                    .len()
                    .min(usize::try_from(reader.left).unwrap_or(usize::MAX));
                let n = reader
                    .response
                    .as_mut()
                    .expect("active range")
                    .read(&mut buffer[..len])?;
                if n == 0 {
                    return Err(io::Error::new(
                        io::ErrorKind::UnexpectedEof,
                        "Truncated Hugging Face range",
                    ));
                }
                reader.left -= n as u64;
                reader.position += n as u64;
                Ok(n)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "Explicit live HTTPS range check, no model import"]
    fn live_hub_range_is_bounded_and_cancellable() {
        let repo =
            crate::huggingface::repository("stabilityai/stable-diffusion-xl-base-1.0", "main", "")
                .unwrap();
        let file = repo
            .siblings
            .iter()
            .find(|f| f.rfilename == "sd_xl_offset_example-lora_1.0.safetensors")
            .unwrap();
        let url = super::super::resolve(&repo, file).unwrap();
        let cancel = AtomicBool::new(false);
        let mut source = reader(url, file, "", &cancel).unwrap();
        let mut header = [0; 8];
        source.read_exact(&mut header).unwrap();
        assert!((2..=16 * 1024 * 1024).contains(&u64::from_le_bytes(header)));
        cancel.store(true, Ordering::Release);
        assert!(source.read(&mut header).is_err());
    }
}
