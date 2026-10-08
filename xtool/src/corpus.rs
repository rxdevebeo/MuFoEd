//! `xtool corpus fetch|verify` — the CC0 corpus, by its lock file
//! (`docs/CC0_CORPUS_MIGRATION_PLAN.md` §4.2).
//!
//! The corpus bytes are never committed; `testdata-lock/cc0.toml` is. `fetch`
//! downloads the documents of one tier into `testdata/` and `verify` checks the
//! ones already there. Both judge a file by its SHA-256 and size and nothing
//! else: a file with the right name and the wrong bytes is a wrong file.
//!
//! `fetch`:
//!
//! * leaves a file that already has the recorded hash alone, so a machine that
//!   has the corpus downloads nothing;
//! * writes to `<name>.part` beside the destination, checks size and hash, and
//!   only then renames over the destination — an interrupted run never leaves a
//!   half-written document under the real name;
//! * follows redirects by hand and refuses any that leaves `https://archive.org`
//!   or `https://*.archive.org`;
//! * retries transport failures with exponential backoff, and exits non-zero
//!   when any document could not be fetched.

use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Component, Path, PathBuf};
use std::process::ExitCode;
use std::thread;
use std::time::Duration;

use sha2::{Digest, Sha256};

const DEFAULT_LOCK: &str = "testdata-lock/cc0.toml";
const DEFAULT_ROOT: &str = "testdata";
const TIERS: &[&str] = &["ci-core", "ci-full"];
/// Attempts per document, the first one included (waits 2, 4, ... 32 s).
const ATTEMPTS: u32 = 6;
/// The wait before a second pass over the documents the first pass could not
/// get: archive.org refuses connections under load and some of its storage
/// nodes answer 500 for a while, so a minute later is often a different answer.
const SECOND_PASS_DELAY: Duration = Duration::from_secs(60);
/// archive.org answers `/download/...` with one redirect to a storage node.
const MAX_REDIRECTS: usize = 8;

/// One `[[doc]]` of the lock file, the fields `fetch` and `verify` need.
#[derive(Debug, Clone, PartialEq, Eq)]
struct LockDoc {
    id: String,
    path: String,
    sha256: String,
    bytes: u64,
    url: String,
    tiers: Vec<String>,
}

/// Entry point for `xtool corpus <fetch|verify> ...`.
pub(crate) fn run(args: &[String]) -> ExitCode {
    match args.first().map(String::as_str) {
        Some("fetch") => fetch(&args[1..]),
        Some("verify") => verify(&args[1..]),
        other => {
            if let Some(other) = other {
                eprintln!("error: unknown corpus command '{other}'");
            }
            eprintln!("{USAGE}");
            ExitCode::from(2)
        }
    }
}

/// The `corpus` lines of `xtool --help`.
pub(crate) const USAGE: &str = "\
corpus fetch    --tier <ci-core|ci-full> [--root <dir>] [--lock <path>] [--allow-failures <n>]\n\
corpus verify   --tier <ci-core|ci-full> [--root <dir>] [--lock <path>]\n\
                (defaults: --root testdata, --lock testdata-lock/cc0.toml)";

/// The tier's documents and where they live, or the exit code of the refusal.
fn load(args: &[String]) -> Result<(PathBuf, String, Vec<LockDoc>), ExitCode> {
    let Some(tier) = super::arg_value(args, "--tier") else {
        eprintln!("error: --tier is required\n{USAGE}");
        return Err(ExitCode::from(2));
    };
    if !TIERS.contains(&tier) {
        eprintln!("error: unknown tier '{tier}' (expected one of {TIERS:?})");
        return Err(ExitCode::from(2));
    }
    let root = PathBuf::from(super::arg_value(args, "--root").unwrap_or(DEFAULT_ROOT));
    let lock = super::arg_value(args, "--lock").unwrap_or(DEFAULT_LOCK);
    let text = fs::read_to_string(lock).map_err(|error| {
        eprintln!("error: read {lock}: {error}");
        ExitCode::from(2)
    })?;
    let docs = parse_lock(&text).map_err(|error| {
        eprintln!("error: {lock}: {error}");
        ExitCode::from(2)
    })?;
    let selected = select(docs, tier);
    if selected.is_empty() {
        eprintln!("error: {lock} has no document in tier {tier}");
        return Err(ExitCode::from(2));
    }
    Ok((root, tier.to_owned(), selected))
}

fn fetch(args: &[String]) -> ExitCode {
    let (root, tier, docs) = match load(args) {
        Ok(loaded) => loaded,
        Err(code) => return code,
    };
    // How many documents may stay unfetched without failing the run: the
    // nightly full tier would otherwise fail on one flaky archive.org node.
    let allowed = super::arg_value(args, "--allow-failures");
    let allowed = match allowed.map(str::parse::<usize>) {
        None => 0,
        Some(Ok(allowed)) => allowed,
        Some(Err(error)) => {
            eprintln!("error: --allow-failures: {error}\n{USAGE}");
            return ExitCode::from(2);
        }
    };
    let agent = agent();
    let (mut fetched, mut present, mut failed) = (0usize, 0usize, Vec::new());
    for doc in &docs {
        let destination = root.join(&doc.path);
        match check_file(&destination, doc) {
            Ok(State::Good) => {
                present += 1;
                continue;
            }
            Ok(State::Missing) => {}
            Ok(State::Wrong(why)) => eprintln!("{}: {why}; downloading again", doc.id),
            Err(error) => {
                eprintln!("error: {}: {error}", doc.id);
                failed.push((doc, error));
                continue;
            }
        }
        match fetch_with_retries(&agent, doc, &destination) {
            Ok(()) => {
                fetched += 1;
                println!(
                    "fetched {} ({} bytes) -> {}",
                    doc.id,
                    doc.bytes,
                    destination.display()
                );
            }
            Err(error) => {
                eprintln!("error: {}: {error}", doc.id);
                failed.push((doc, error));
            }
        }
    }
    if !failed.is_empty() {
        eprintln!(
            "{} document(s) failed; a second pass in {}s",
            failed.len(),
            SECOND_PASS_DELAY.as_secs()
        );
        thread::sleep(SECOND_PASS_DELAY);
        let mut still = Vec::new();
        for (doc, first) in failed {
            match fetch_with_retries(&agent, doc, &root.join(&doc.path)) {
                Ok(()) => {
                    fetched += 1;
                    println!("fetched {} on the second pass", doc.id);
                }
                Err(error) => {
                    eprintln!("error: {}: {error} (first pass: {first})", doc.id);
                    still.push((doc, error));
                }
            }
        }
        failed = still;
    }
    println!(
        "corpus fetch --tier {tier}: {} document(s): {fetched} fetched, {present} already present, {} failed",
        docs.len(),
        failed.len()
    );
    for (doc, error) in &failed {
        println!("  unfetched: {} ({}): {error}", doc.id, doc.url);
    }
    if failed.len() <= allowed {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    }
}

fn verify(args: &[String]) -> ExitCode {
    let (root, tier, docs) = match load(args) {
        Ok(loaded) => loaded,
        Err(code) => return code,
    };
    let (mut good, mut missing, mut wrong) = (0usize, 0usize, 0usize);
    for doc in &docs {
        let path = root.join(&doc.path);
        match check_file(&path, doc) {
            Ok(State::Good) => good += 1,
            Ok(State::Missing) => {
                missing += 1;
                eprintln!("missing: {} ({})", doc.id, path.display());
            }
            Ok(State::Wrong(why)) => {
                wrong += 1;
                eprintln!("MISMATCH: {} ({}): {why}", doc.id, path.display());
            }
            Err(error) => {
                wrong += 1;
                eprintln!("error: {}: {error}", doc.id);
            }
        }
    }
    println!(
        "corpus verify --tier {tier}: {} document(s): {good} ok, {missing} missing, {wrong} mismatched",
        docs.len()
    );
    if missing == 0 && wrong == 0 {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    }
}

fn parse_lock(text: &str) -> Result<Vec<LockDoc>, String> {
    let value: toml::Value =
        toml::from_str(text).map_err(|error| format!("invalid TOML: {error}"))?;
    match value.get("schema").and_then(toml::Value::as_integer) {
        Some(1) => {}
        other => return Err(format!("unsupported lock schema {other:?} (expected 1)")),
    }
    let docs = value
        .get("doc")
        .and_then(toml::Value::as_array)
        .ok_or("the lock has no [[doc]] entries")?;
    let docs = docs
        .iter()
        .enumerate()
        .map(|(index, doc)| parse_doc(index, doc))
        .collect::<Result<Vec<_>, _>>()?;
    let mut ids: Vec<&str> = docs.iter().map(|doc| doc.id.as_str()).collect();
    ids.sort_unstable();
    if let Some(id) = ids.windows(2).find_map(|pair| match pair {
        [first, second] if first == second => Some(*first),
        _ => None,
    }) {
        return Err(format!("duplicate id {id}"));
    }
    Ok(docs)
}

fn parse_doc(index: usize, doc: &toml::Value) -> Result<LockDoc, String> {
    let text = |key: &str| -> Result<String, String> {
        doc.get(key)
            .and_then(toml::Value::as_str)
            .map(str::to_owned)
            .ok_or_else(|| format!("doc #{index}: missing string `{key}`"))
    };
    let id = text("id")?;
    let path = text("path")?;
    check_relative(&path).map_err(|error| format!("{id}: {error}"))?;
    let sha256 = text("sha256")?;
    if sha256.len() != 64
        || !sha256
            .bytes()
            .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
    {
        return Err(format!("{id}: sha256 is not 64 lowercase hex digits"));
    }
    let bytes = doc
        .get("bytes")
        .and_then(toml::Value::as_integer)
        .and_then(|bytes| u64::try_from(bytes).ok())
        .ok_or_else(|| format!("{id}: missing non-negative integer `bytes`"))?;
    let url = text("url")?;
    let tiers = doc
        .get("tiers")
        .and_then(toml::Value::as_array)
        .ok_or_else(|| format!("{id}: missing array `tiers`"))?
        .iter()
        .map(|tier| {
            tier.as_str()
                .map(str::to_owned)
                .ok_or_else(|| format!("{id}: a tier is not a string"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(LockDoc {
        id,
        path,
        sha256,
        bytes,
        url,
        tiers,
    })
}

/// A lock path is a relative `/`-separated path that stays under the root.
fn check_relative(path: &str) -> Result<(), String> {
    let plain = !path.is_empty()
        && !path.contains('\\')
        && !path.contains(':')
        && path
            .split('/')
            .all(|segment| !segment.is_empty() && segment != "." && segment != "..")
        && Path::new(path)
            .components()
            .all(|component| matches!(component, Component::Normal(_)));
    if plain {
        Ok(())
    } else {
        Err(format!("path `{path}` is not a plain relative path"))
    }
}

fn select(docs: Vec<LockDoc>, tier: &str) -> Vec<LockDoc> {
    docs.into_iter()
        .filter(|doc| doc.tiers.iter().any(|name| name == tier))
        .collect()
}

#[derive(Debug, PartialEq, Eq)]
enum State {
    Missing,
    Good,
    Wrong(String),
}

fn check_file(path: &Path, doc: &LockDoc) -> Result<State, String> {
    let mut file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(State::Missing),
        Err(error) => return Err(format!("open {}: {error}", path.display())),
    };
    let (size, digest) =
        hash_reader(&mut file).map_err(|error| format!("read {}: {error}", path.display()))?;
    if size != doc.bytes {
        return Ok(State::Wrong(format!(
            "{size} bytes, the lock records {}",
            doc.bytes
        )));
    }
    if digest != doc.sha256 {
        return Ok(State::Wrong(format!(
            "sha256 {digest}, the lock records {}",
            doc.sha256
        )));
    }
    Ok(State::Good)
}

fn hash_reader(reader: &mut dyn Read) -> io::Result<(u64, String)> {
    let mut hasher = Sha256::new();
    let mut size = 0u64;
    let mut buffer = vec![0u8; 64 * 1024];
    loop {
        let read = match reader.read(&mut buffer) {
            Ok(0) => break,
            Ok(read) => read,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(error),
        };
        size += u64::try_from(read).unwrap_or(u64::MAX);
        hasher.update(&buffer[..read]);
    }
    Ok((size, hex(&hasher.finalize())))
}

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    let mut text = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        let _ = write!(text, "{byte:02x}");
    }
    text
}

/// A failed attempt, and whether another attempt could help.
#[derive(Debug)]
struct Failure {
    message: String,
    retry: bool,
}

impl Failure {
    fn transient(message: String) -> Self {
        Self {
            message,
            retry: true,
        }
    }

    fn fatal(message: String) -> Self {
        Self {
            message,
            retry: false,
        }
    }
}

fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        // Redirects are followed by hand so every hop is checked against the
        // archive.org allow-list before it is requested.
        .max_redirects(0)
        .http_status_as_error(false)
        .timeout_connect(Some(Duration::from_secs(30)))
        .timeout_global(Some(Duration::from_secs(900)))
        .user_agent("strict-ooxml-xtool-corpus-fetch")
        .build()
        .into()
}

fn fetch_with_retries(
    agent: &ureq::Agent,
    doc: &LockDoc,
    destination: &Path,
) -> Result<(), String> {
    let mut delay = Duration::from_secs(2);
    let mut attempt = 1;
    loop {
        match download(agent, doc, destination) {
            Ok(()) => return Ok(()),
            Err(failure) if failure.retry && attempt < ATTEMPTS => {
                eprintln!(
                    "{}: attempt {attempt}/{ATTEMPTS} failed: {}; retrying in {}s",
                    doc.id,
                    failure.message,
                    delay.as_secs()
                );
                thread::sleep(delay);
                delay *= 2;
                attempt += 1;
            }
            Err(failure) => return Err(failure.message),
        }
    }
}

fn download(agent: &ureq::Agent, doc: &LockDoc, destination: &Path) -> Result<(), Failure> {
    let mut url = doc.url.clone();
    check_url(&url).map_err(Failure::fatal)?;
    for _ in 0..=MAX_REDIRECTS {
        let mut response = agent
            .get(url.as_str())
            .call()
            .map_err(|error| Failure::transient(format!("GET {url}: {error}")))?;
        let status = response.status();
        if status.is_redirection() {
            let location = response
                .headers()
                .get("location")
                .and_then(|value| value.to_str().ok())
                .ok_or_else(|| {
                    Failure::transient(format!("GET {url}: {status} without a Location"))
                })?;
            let next = resolve(&url, location).map_err(Failure::fatal)?;
            check_url(&next).map_err(Failure::fatal)?;
            url = next;
            continue;
        }
        if status.as_u16() != 200 {
            // 404/403/410 will not change on a retry; anything else might.
            let retry = !matches!(status.as_u16(), 403 | 404 | 410);
            let message = format!("GET {url}: HTTP {status}");
            return Err(Failure { message, retry });
        }
        return store(&mut response.body_mut().as_reader(), doc, destination);
    }
    Err(Failure::fatal(format!(
        "more than {MAX_REDIRECTS} redirects from {}",
        doc.url
    )))
}

/// The host of an `https://` URL, lower-cased, without port; `None` for any
/// other scheme or a URL with credentials.
fn https_host(url: &str) -> Option<String> {
    let rest = url.strip_prefix("https://")?;
    let authority = rest.split(['/', '?', '#']).next()?;
    if authority.contains('@') {
        return None;
    }
    let host = authority.split(':').next()?;
    (!host.is_empty()).then(|| host.to_ascii_lowercase())
}

fn check_url(url: &str) -> Result<(), String> {
    let host = https_host(url).ok_or_else(|| format!("refusing `{url}`: not a plain https URL"))?;
    if host == "archive.org" || host.ends_with(".archive.org") {
        Ok(())
    } else {
        Err(format!("refusing `{url}`: host {host} is not archive.org"))
    }
}

/// Resolves a `Location` against the URL that returned it. Only absolute
/// `https://` and origin-relative (`/path`) locations are accepted.
fn resolve(base: &str, location: &str) -> Result<String, String> {
    if location.starts_with("https://") {
        return Ok(location.to_owned());
    }
    if location.starts_with('/') && !location.starts_with("//") {
        let host_end = base
            .strip_prefix("https://")
            .and_then(|rest| rest.find('/'))
            .map_or(base.len(), |index| index + "https://".len());
        return Ok(format!("{}{location}", &base[..host_end]));
    }
    Err(format!("refusing redirect from {base} to `{location}`"))
}

/// Streams `body` to `<destination>.part`, checks size and hash, renames.
fn store(body: &mut dyn Read, doc: &LockDoc, destination: &Path) -> Result<(), Failure> {
    let parent = destination
        .parent()
        .ok_or_else(|| Failure::fatal(format!("{} has no parent", destination.display())))?;
    fs::create_dir_all(parent)
        .map_err(|error| Failure::fatal(format!("create {}: {error}", parent.display())))?;
    let mut name = destination.as_os_str().to_owned();
    name.push(".part");
    let partial = PathBuf::from(name);
    let result = write_verified(body, doc, &partial).and_then(|()| {
        fs::rename(&partial, destination).map_err(|error| {
            Failure::fatal(format!("rename to {}: {error}", destination.display()))
        })
    });
    if result.is_err() {
        let _ = fs::remove_file(&partial);
    }
    result
}

fn write_verified(body: &mut dyn Read, doc: &LockDoc, partial: &Path) -> Result<(), Failure> {
    let io_error = |what: &str, error: &io::Error| {
        Failure::transient(format!("{what} {}: {error}", partial.display()))
    };
    let mut file = File::create(partial).map_err(|error| io_error("create", &error))?;
    let mut hasher = Sha256::new();
    let mut size = 0u64;
    let mut buffer = vec![0u8; 64 * 1024];
    loop {
        let read = match body.read(&mut buffer) {
            Ok(0) => break,
            Ok(read) => read,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(Failure::transient(format!("download: {error}"))),
        };
        size += u64::try_from(read).unwrap_or(u64::MAX);
        if size > doc.bytes {
            return Err(Failure::transient(format!(
                "the server sent more than the {} bytes the lock records",
                doc.bytes
            )));
        }
        hasher.update(&buffer[..read]);
        file.write_all(&buffer[..read])
            .map_err(|error| io_error("write", &error))?;
    }
    file.sync_all().map_err(|error| io_error("sync", &error))?;
    drop(file);
    if size != doc.bytes {
        return Err(Failure::transient(format!(
            "received {size} bytes, the lock records {}",
            doc.bytes
        )));
    }
    let digest = hex(&hasher.finalize());
    if digest != doc.sha256 {
        return Err(Failure::transient(format!(
            "received sha256 {digest}, the lock records {}",
            doc.sha256
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"
schema = 1

[[doc]]
id = "cc0/001"
path = "CC0/001_a.docx"
sha256 = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
bytes = 3
url = "https://archive.org/download/x/a.docx"
license = "CC0-1.0"
tiers = ["ci-core", "ci-full"]
features = ["tbl"]

[[doc]]
id = "cc0/002"
path = "CC0/002_b.docx"
sha256 = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
bytes = 3
url = "https://archive.org/download/x/b.docx"
license = "CC0-1.0"
tiers = ["ci-full"]
features = []
"#;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("xtool-corpus-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("create temp dir");
        dir
    }

    #[test]
    fn lock_parses_and_tiers_select() {
        let docs = parse_lock(SAMPLE).expect("sample lock");
        assert_eq!(docs.len(), 2);
        assert_eq!(docs[0].path, "CC0/001_a.docx");
        assert_eq!(select(docs.clone(), "ci-core").len(), 1);
        assert_eq!(select(docs, "ci-full").len(), 2);
    }

    #[test]
    fn committed_lock_parses() {
        let text = include_str!("../../testdata-lock/cc0.toml");
        let docs = parse_lock(text).expect("committed lock");
        assert_eq!(docs.len(), 300);
        assert_eq!(select(docs.clone(), "ci-core").len(), 27);
        for doc in &docs {
            check_url(&doc.url).expect("every lock URL is on archive.org");
        }
    }

    #[test]
    fn lock_rejects_bad_input() {
        assert!(parse_lock("").is_err());
        assert!(parse_lock("schema = 2\n[[doc]]\n").is_err());
        let escape = SAMPLE.replace("CC0/001_a.docx", "../001_a.docx");
        assert!(parse_lock(&escape).is_err());
        let duplicate = SAMPLE.replace("cc0/002", "cc0/001");
        assert!(parse_lock(&duplicate).is_err());
        let short = SAMPLE.replacen("ba7816bf", "ba78", 1);
        assert!(parse_lock(&short).is_err());
    }

    #[test]
    fn relative_paths() {
        assert!(check_relative("CC0/a.docx").is_ok());
        for bad in ["", "/etc/x", "a/../b", "./a", "a//b", "C:/x", "a\\b"] {
            assert!(check_relative(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn only_archive_org_over_https() {
        assert!(check_url("https://archive.org/download/a/b.docx").is_ok());
        assert!(check_url("https://ia800204.us.archive.org/1/items/a/b.docx").is_ok());
        assert!(check_url("https://ARCHIVE.ORG:443/x").is_ok());
        for bad in [
            "http://archive.org/download/a",
            "https://archive.org.evil.example/a",
            "https://evilarchive.org/a",
            "https://user@archive.org/a",
            "ftp://archive.org/a",
        ] {
            assert!(check_url(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn redirects_resolve() {
        let base = "https://archive.org/download/a/b.docx";
        assert_eq!(
            resolve(base, "https://ia1.us.archive.org/x").as_deref(),
            Ok("https://ia1.us.archive.org/x")
        );
        assert_eq!(
            resolve(base, "/items/a/b.docx").as_deref(),
            Ok("https://archive.org/items/a/b.docx")
        );
        assert!(resolve(base, "//evil.example/x").is_err());
        assert!(resolve(base, "relative/x").is_err());
    }

    #[test]
    fn store_checks_size_and_hash_and_is_atomic() {
        let dir = temp_dir("store");
        let docs = parse_lock(SAMPLE).expect("sample lock");
        let doc = &docs[0]; // sha256("abc")
        let destination = dir.join(&doc.path);

        let wrong = store(&mut &b"abd"[..], doc, &destination).expect_err("hash mismatch");
        assert!(wrong.message.contains("sha256"), "{}", wrong.message);
        assert!(!destination.exists());
        let long = store(&mut &b"abcd"[..], doc, &destination).expect_err("too long");
        assert!(long.message.contains("more than"), "{}", long.message);
        let short = store(&mut &b"ab"[..], doc, &destination).expect_err("too short");
        assert!(
            short.message.contains("received 2 bytes"),
            "{}",
            short.message
        );
        assert_eq!(check_file(&destination, doc), Ok(State::Missing));

        store(&mut &b"abc"[..], doc, &destination).expect("good bytes");
        assert_eq!(check_file(&destination, doc), Ok(State::Good));
        let mut partial = destination.as_os_str().to_owned();
        partial.push(".part");
        assert!(!PathBuf::from(partial).exists());

        fs::write(&destination, b"abx").expect("tamper");
        assert!(matches!(check_file(&destination, doc), Ok(State::Wrong(_))));
        let _ = fs::remove_dir_all(&dir);
    }
}
