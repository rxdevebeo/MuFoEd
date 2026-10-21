//! Demo-only preference storage. The embeddable widget performs no implicit filesystem writes.
use image_editor::{ui::EditorSettings, EditError};
use std::{
    io::{Read, Write},
    path::PathBuf,
    time::{Duration, Instant},
};

const MAX_BYTES: usize = 4 * 1024 * 1024;
pub(super) struct Preferences {
    path: PathBuf,
    saved: EditorSettings,
    initialized: bool,
    pending: Option<(EditorSettings, Instant)>,
}
pub(super) fn path() -> PathBuf {
    std::env::var_os("IMAGE_STUDIO_SETTINGS").map_or_else(
        || {
            std::env::var_os("APPDATA")
                .map_or_else(|| PathBuf::from(".runtime"), PathBuf::from)
                .join("MuFoEd")
                .join("ImageStudio")
                .join("settings.json")
        },
        PathBuf::from,
    )
}
pub(super) fn load(path: &std::path::Path) -> Result<Option<EditorSettings>, EditError> {
    let file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.into()),
    };
    let mut bytes = Vec::new();
    file.take((MAX_BYTES + 1) as u64).read_to_end(&mut bytes)?;
    if bytes.len() > MAX_BYTES {
        return Err(EditError::Invalid(
            "Preferences file exceeds size limit".into(),
        ));
    }
    let settings: EditorSettings = serde_json::from_slice(&bytes)?;
    settings.validate()?;
    Ok(Some(settings))
}
impl Preferences {
    pub(super) fn new(path: PathBuf, saved: EditorSettings) -> Self {
        let initialized = path.is_file();
        Self {
            initialized,
            path,
            saved,
            pending: None,
        }
    }
    pub(super) fn poll(&mut self, current: EditorSettings) -> Result<(), EditError> {
        if self.initialized && current == self.saved {
            self.pending = None;
            return Ok(());
        }
        if self.pending.as_ref().is_none_or(|(s, _)| *s != current) {
            self.pending = Some((current, Instant::now()));
        }
        if self
            .pending
            .as_ref()
            .is_some_and(|(_, t)| t.elapsed() >= Duration::from_millis(500))
        {
            self.flush()?;
        }
        Ok(())
    }
    pub(super) fn flush(&mut self) -> Result<(), EditError> {
        if let Some((settings, _)) = &self.pending {
            let parent = self
                .path
                .parent()
                .filter(|p| !p.as_os_str().is_empty())
                .unwrap_or_else(|| std::path::Path::new("."));
            std::fs::create_dir_all(parent)?;
            let mut file = tempfile::NamedTempFile::new_in(parent)?;
            file.write_all(&serde_json::to_vec_pretty(settings)?)?;
            file.as_file().sync_all()?;
            file.persist(&self.path).map_err(|e| e.error)?;
            self.saved = settings.clone();
            self.initialized = true;
            self.pending = None;
        }
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn saves_and_reopens_configuration_atomically() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("preferences.json");
        assert!(load(&path).unwrap().is_none());
        let settings = image_editor::ui::ImageEditor::default().settings();
        let mut storage = Preferences::new(path.clone(), settings.clone());
        storage.poll(settings.clone()).unwrap();
        storage.flush().unwrap();
        assert_eq!(load(&path).unwrap(), Some(settings.clone()));
        let mut value = serde_json::to_value(settings).unwrap();
        value["request"]["model"] = "local/next-checkpoint".into();
        let next: EditorSettings = serde_json::from_value(value).unwrap();
        storage.poll(next.clone()).unwrap();
        storage.flush().unwrap();
        assert_eq!(load(&path).unwrap(), Some(next));
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    }
    #[test]
    fn invalid_and_oversized_preferences_are_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        std::fs::write(&path, b"not json").unwrap();
        assert!(load(&path).is_err());
        std::fs::write(&path, vec![b' '; MAX_BYTES + 1]).unwrap();
        assert!(load(&path).is_err());
    }
}
