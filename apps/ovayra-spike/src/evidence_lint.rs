use std::{
    collections::BTreeSet,
    fmt::{self, Write as _},
    fs,
    io::Read,
    path::{Path, PathBuf},
};

use anyhow::Result;
use serde::de::{self, Error as _, MapAccess, SeqAccess, Visitor};
use serde::{Deserialize, Deserializer};
use sha2::{Digest, Sha256};
use spike_contracts::{
    Evidence, PhaseZeroProof, PreviewProof, ProofComponent, ProofPayload, TargetId,
};
use unicode_normalization::UnicodeNormalization;

const MAX_FILE_BYTES: u64 = 1_048_576;
const MAX_FILES: usize = 1_024;
const MAX_DIRECTORIES: usize = 128;
const MAX_DEPTH: usize = 32;
const MAX_TOTAL_BYTES: u64 = 8 * MAX_FILE_BYTES;
const FORBIDDEN_KEY_PARTS: &[&str] = &[
    "apikey",
    "token",
    "secret",
    "password",
    "uploadurl",
    "prompt",
    "result",
    "mediapath",
    "filename",
];

#[derive(Clone, Copy)]
enum Category {
    MissingDirectory,
    NotDirectory,
    Symlink,
    NonRegular,
    TooManyFiles,
    Oversized,
    BinaryOrInvalidText,
    UnsupportedFile,
    InvalidJson,
    DuplicateKey,
    InvalidEvidence,
    ForbiddenKey,
    ApiKey,
    UploadHeader,
    PrivateKey,
    UploadUrl,
    UrlCredential,
    HomePath,
    UnsafeName,
    MaxDepth,
    TooManyDirectories,
    ChangedEntry,
    Io,
}

impl Category {
    const fn as_str(self) -> &'static str {
        match self {
            Self::MissingDirectory => "missing-directory",
            Self::NotDirectory => "not-directory",
            Self::Symlink => "symlink",
            Self::NonRegular => "non-regular",
            Self::TooManyFiles => "too-many-files",
            Self::Oversized => "oversized",
            Self::BinaryOrInvalidText => "binary-or-invalid-text",
            Self::UnsupportedFile => "unsupported-file",
            Self::InvalidJson => "invalid-json",
            Self::DuplicateKey => "duplicate-key",
            Self::InvalidEvidence => "invalid-evidence",
            Self::ForbiddenKey => "forbidden-key",
            Self::ApiKey => "api-key",
            Self::UploadHeader => "upload-header",
            Self::PrivateKey => "private-key",
            Self::UploadUrl => "upload-url",
            Self::UrlCredential => "url-credential",
            Self::HomePath => "home-path",
            Self::UnsafeName => "unsafe-name",
            Self::MaxDepth => "max-depth",
            Self::TooManyDirectories => "too-many-directories",
            Self::ChangedEntry => "changed-entry",
            Self::Io => "io",
        }
    }
}

struct LintError {
    entry: String,
    category: Category,
}

impl LintError {
    fn new(relative: impl AsRef<Path>, category: Category) -> Self {
        Self {
            entry: entry_id(relative.as_ref()),
            category,
        }
    }
}

impl fmt::Display for LintError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "evidence lint rejected entry={}: {}",
            self.entry,
            self.category.as_str()
        )
    }
}

impl fmt::Debug for LintError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, formatter)
    }
}

impl std::error::Error for LintError {}

/// Bytes and digest obtained from the same regular-file handle that passed the
/// bounded redaction lint. Consumers must parse these bytes directly.
pub(crate) struct VerifiedEvidence {
    pub(crate) bytes: Vec<u8>,
    pub(crate) sha256: String,
}

/// Scans only bounded regular files below `directory`, never follows symlinks,
/// and reports a stable relative-path/category diagnostic on rejection.
pub(crate) fn lint_dir(directory: &Path, text_mode: bool) -> Result<()> {
    lint_dir_quiet(directory, text_mode)?;
    println!("EVIDENCE_LINT=PASS");
    Ok(())
}

/// Runs the same bounded traversal and redaction checks without printing a
/// success token. The final acceptance gate uses this so a rejected gate never
/// emits a misleading success marker.
pub(crate) fn lint_dir_quiet(directory: &Path, text_mode: bool) -> Result<()> {
    let _ = lint_verified(directory, text_mode)?;
    Ok(())
}

/// Performs one bounded traversal and returns the exact verified JSON bytes.
/// This closes the lint-to-gate TOCTOU window.
pub(crate) fn lint_verified(directory: &Path, text_mode: bool) -> Result<Vec<VerifiedEvidence>> {
    let metadata = match fs::symlink_metadata(directory) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Err(LintError::new(".", Category::MissingDirectory).into());
        }
        Err(_) => return Err(LintError::new(".", Category::Io).into()),
    };
    if metadata.file_type().is_symlink() {
        return Err(LintError::new(".", Category::Symlink).into());
    }
    if !metadata.is_dir() {
        return Err(LintError::new(".", Category::NotDirectory).into());
    }

    let canonical_root =
        fs::canonicalize(directory).map_err(|_| LintError::new(".", Category::Io))?;
    let mut files = Vec::new();
    let mut directories = 1;
    collect_regular_files(
        directory,
        &canonical_root,
        directory,
        0,
        &mut directories,
        &mut files,
    )?;
    if files.len() > MAX_FILES {
        return Err(LintError::new(".", Category::TooManyFiles).into());
    }
    let mut total_bytes = 0_u64;
    let mut verified = Vec::new();
    for file in files {
        let bytes = lint_file(directory, &canonical_root, &file, text_mode)?;
        total_bytes = total_bytes.saturating_add(u64::try_from(bytes.len()).unwrap_or(u64::MAX));
        if total_bytes > MAX_TOTAL_BYTES {
            return Err(LintError::new(".", Category::Oversized).into());
        }
        if !bytes.is_empty() {
            let digest = Sha256::digest(&bytes);
            let mut sha256 = String::with_capacity(64);
            for byte in digest {
                let _ = write!(sha256, "{byte:02x}");
            }
            verified.push(VerifiedEvidence { bytes, sha256 });
        }
    }
    Ok(verified)
}

/// Validates the documented desktop preview threshold from a strict schema-v2
/// `Preview` proof, never from generic measurements or a console line.
pub(crate) fn verify_preview(file: &Path, expected_target: &str) -> Result<()> {
    let contents = fs::read_to_string(file).map_err(|_| LintError::new(".", Category::Io))?;
    let proof = PhaseZeroProof::from_json(&contents)
        .map_err(|_| LintError::new(".", Category::InvalidEvidence))?;
    let expected_target = TargetId::new(expected_target)
        .map_err(|_| anyhow::anyhow!("expected target must be a supported Phase 0 target"))?;
    if proof.component != ProofComponent::Preview || proof.row.target != expected_target {
        anyhow::bail!("preview evidence did not report pass");
    }
    let ProofPayload::Preview(value) = proof.proof else {
        anyhow::bail!("preview evidence did not contain a preview proof");
    };
    if !preview_accepted(&value) {
        anyhow::bail!("preview evidence did not meet the documented threshold");
    }
    println!("PREVIEW_EVIDENCE=PASS");
    Ok(())
}

fn preview_accepted(value: &PreviewProof) -> bool {
    value.requested_duration_ms == 120_000
        && value.measured_duration_ms >= 120_000
        && (23_000..=25_000).contains(&value.milli_fps)
        && value.p95_ms <= 100
        && value.rss_growth_mib <= 64
        && value.frames_read > 0
        && value.frames_applied > 0
        && value.frames_applied + value.frames_dropped <= value.frames_read
        && value.hidden
        && value.restored
        && value.event_loop_errors == 0
        && value.stream_errors == 0
}

fn collect_regular_files(
    root: &Path,
    canonical_root: &Path,
    current: &Path,
    depth: usize,
    directories: &mut usize,
    files: &mut Vec<PathBuf>,
) -> Result<()> {
    if depth > MAX_DEPTH {
        return Err(LintError::new(relative(root, current), Category::MaxDepth).into());
    }
    let current_canonical = fs::canonicalize(current)
        .map_err(|_| LintError::new(relative(root, current), Category::Io))?;
    if !current_canonical.starts_with(canonical_root) {
        return Err(LintError::new(relative(root, current), Category::ChangedEntry).into());
    }
    let entries =
        fs::read_dir(current).map_err(|_| LintError::new(relative(root, current), Category::Io))?;
    for entry in entries {
        let entry = entry.map_err(|_| LintError::new(relative(root, current), Category::Io))?;
        let path = entry.path();
        let metadata = fs::symlink_metadata(&path)
            .map_err(|_| LintError::new(relative(root, &path), Category::Io))?;
        if metadata.file_type().is_symlink() {
            return Err(LintError::new(relative(root, &path), Category::Symlink).into());
        }
        if metadata.is_dir() {
            *directories += 1;
            if *directories > MAX_DIRECTORIES {
                return Err(LintError::new(".", Category::TooManyDirectories).into());
            }
            collect_regular_files(root, canonical_root, &path, depth + 1, directories, files)?;
        } else if metadata.is_file() {
            files.push(path);
            if files.len() > MAX_FILES {
                return Err(LintError::new(".", Category::TooManyFiles).into());
            }
        } else {
            return Err(LintError::new(relative(root, &path), Category::NonRegular).into());
        }
    }
    Ok(())
}

fn lint_file(root: &Path, canonical_root: &Path, file: &Path, text_mode: bool) -> Result<Vec<u8>> {
    let relative = relative(root, file);
    if !text_mode && relative == Path::new(".gitkeep") {
        return Ok(Vec::new());
    }
    if unsafe_name(&relative) {
        return Err(LintError::new(&relative, Category::UnsafeName).into());
    }
    let before = fs::symlink_metadata(file).map_err(|_| LintError::new(&relative, Category::Io))?;
    if before.file_type().is_symlink() || !before.is_file() {
        return Err(LintError::new(&relative, Category::ChangedEntry).into());
    }
    let mut opened = fs::File::open(file).map_err(|_| LintError::new(&relative, Category::Io))?;
    let opened_before = opened
        .metadata()
        .map_err(|_| LintError::new(&relative, Category::Io))?;
    let after = fs::symlink_metadata(file).map_err(|_| LintError::new(&relative, Category::Io))?;
    let canonical_file =
        fs::canonicalize(file).map_err(|_| LintError::new(&relative, Category::Io))?;
    if after.file_type().is_symlink()
        || !same_identity(&before, &opened_before)
        || !same_identity(&opened_before, &after)
        || !canonical_file.starts_with(canonical_root)
    {
        return Err(LintError::new(&relative, Category::ChangedEntry).into());
    }
    if !text_mode && file.extension().and_then(|extension| extension.to_str()) != Some("json") {
        return Err(LintError::new(relative, Category::UnsupportedFile).into());
    }
    let bytes = read_bounded(&mut opened).map_err(|error| {
        LintError::new(
            &relative,
            if error.kind() == std::io::ErrorKind::FileTooLarge {
                Category::Oversized
            } else {
                Category::Io
            },
        )
    })?;
    let opened_after = opened
        .metadata()
        .map_err(|_| LintError::new(&relative, Category::Io))?;
    let path_after =
        fs::symlink_metadata(file).map_err(|_| LintError::new(&relative, Category::Io))?;
    let canonical_after =
        fs::canonicalize(file).map_err(|_| LintError::new(&relative, Category::Io))?;
    if path_after.file_type().is_symlink()
        || !path_after.is_file()
        || !same_identity(&before, &opened_after)
        || !same_identity(&opened_after, &path_after)
        || opened_after.len() != u64::try_from(bytes.len()).unwrap_or(u64::MAX)
        || !canonical_after.starts_with(canonical_root)
    {
        return Err(LintError::new(&relative, Category::ChangedEntry).into());
    }
    if bytes.contains(&0) {
        return Err(LintError::new(relative, Category::BinaryOrInvalidText).into());
    }
    let contents = std::str::from_utf8(&bytes)
        .map_err(|_| LintError::new(&relative, Category::BinaryOrInvalidText))?;
    if text_mode {
        scan_text(contents).map_err(|category| LintError::new(relative, category))?;
        return Ok(bytes);
    }
    parse_and_scan_json(contents).map_err(|category| LintError::new(&relative, category))?;
    if Evidence::from_json(contents).is_err() && PhaseZeroProof::from_json(contents).is_err() {
        return Err(LintError::new(relative, Category::InvalidEvidence).into());
    }
    Ok(bytes)
}

fn read_bounded<R: Read>(mut reader: R) -> std::io::Result<Vec<u8>> {
    let capacity = usize::try_from(MAX_FILE_BYTES + 1).expect("Phase 0 lint cap fits usize");
    let mut bytes = Vec::with_capacity(capacity);
    reader
        .by_ref()
        .take(MAX_FILE_BYTES + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > usize::try_from(MAX_FILE_BYTES).expect("Phase 0 lint cap fits usize") {
        return Err(std::io::Error::from(std::io::ErrorKind::FileTooLarge));
    }
    Ok(bytes)
}

fn entry_id(relative: &Path) -> String {
    if relative == Path::new(".") {
        return "root".to_owned();
    }
    let digest = Sha256::digest(relative.as_os_str().as_encoded_bytes());
    let mut output = String::with_capacity(12);
    for byte in digest.iter().take(6) {
        let _ = write!(output, "{byte:02x}");
    }
    output
}

fn unsafe_name(relative: &Path) -> bool {
    relative.components().any(|component| {
        let name = component.as_os_str().to_string_lossy();
        !name.is_ascii() || scan_text(&name).is_err() || forbidden_key(&name)
    })
}

#[cfg(unix)]
fn same_identity(left: &fs::Metadata, right: &fs::Metadata) -> bool {
    use std::os::unix::fs::MetadataExt as _;
    left.dev() == right.dev() && left.ino() == right.ino()
}

#[cfg(windows)]
fn same_identity(left: &fs::Metadata, right: &fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt as _;
    left.file_attributes() == right.file_attributes()
        && left.creation_time() == right.creation_time()
        && left.file_size() == right.file_size()
        && left.last_write_time() == right.last_write_time()
}

#[cfg(not(any(unix, windows)))]
fn same_identity(left: &fs::Metadata, right: &fs::Metadata) -> bool {
    left.len() == right.len() && left.file_type() == right.file_type()
}

fn relative(root: &Path, path: &Path) -> PathBuf {
    path.strip_prefix(root)
        .unwrap_or(Path::new("."))
        .to_path_buf()
}

fn parse_and_scan_json(contents: &str) -> std::result::Result<(), Category> {
    match serde_json::from_str::<SafeJson>(contents) {
        Ok(_) => Ok(()),
        Err(error) => category_from_json_error(&error),
    }
}

fn category_from_json_error(error: &serde_json::Error) -> std::result::Result<(), Category> {
    for category in [
        Category::DuplicateKey,
        Category::ForbiddenKey,
        Category::ApiKey,
        Category::UploadHeader,
        Category::PrivateKey,
        Category::UploadUrl,
        Category::UrlCredential,
        Category::HomePath,
    ] {
        if error.to_string().contains(category.as_str()) {
            return Err(category);
        }
    }
    Err(Category::InvalidJson)
}

struct SafeJson;

impl<'de> Deserialize<'de> for SafeJson {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_any(SafeJsonVisitor)
    }
}

struct SafeJsonVisitor;

impl<'de> Visitor<'de> for SafeJsonVisitor {
    type Value = SafeJson;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a JSON value")
    }

    fn visit_bool<E>(self, _: bool) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        Ok(SafeJson)
    }

    fn visit_i64<E>(self, _: i64) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        Ok(SafeJson)
    }

    fn visit_u64<E>(self, _: u64) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        Ok(SafeJson)
    }

    fn visit_f64<E>(self, _: f64) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        Ok(SafeJson)
    }

    fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        scan_text(value).map_err(|category| E::custom(category.as_str()))?;
        Ok(SafeJson)
    }

    fn visit_string<E>(self, value: String) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        self.visit_str(&value)
    }

    fn visit_none<E>(self) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        Ok(SafeJson)
    }

    fn visit_unit<E>(self) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        Ok(SafeJson)
    }

    fn visit_seq<A>(self, mut sequence: A) -> Result<Self::Value, A::Error>
    where
        A: SeqAccess<'de>,
    {
        while sequence.next_element::<SafeJson>()?.is_some() {}
        Ok(SafeJson)
    }

    fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
    where
        A: MapAccess<'de>,
    {
        let mut keys = BTreeSet::new();
        while let Some(key) = map.next_key::<String>()? {
            if !keys.insert(key.clone()) {
                return Err(A::Error::custom(Category::DuplicateKey.as_str()));
            }
            if forbidden_key(&key) {
                return Err(A::Error::custom(Category::ForbiddenKey.as_str()));
            }
            map.next_value::<SafeJson>()?;
        }
        Ok(SafeJson)
    }
}

fn forbidden_key(key: &str) -> bool {
    let normalized: String = key.nfkc().flat_map(char::to_lowercase).collect();
    if !normalized.is_ascii() {
        return true;
    }
    let normalized: String = normalized
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .collect();
    FORBIDDEN_KEY_PARTS
        .iter()
        .any(|forbidden| normalized.contains(forbidden))
}

fn scan_text(value: &str) -> std::result::Result<(), Category> {
    let lower = value.to_ascii_lowercase();
    if ["aiza", "sk-", "ya29.", "akia"]
        .iter()
        .any(|prefix| lower.contains(prefix))
        || lower.contains("api_key=")
        || lower.contains("api-key=")
        || lower.contains("authorization: bearer")
        || lower.contains("bearer ")
    {
        return Err(Category::ApiKey);
    }
    if lower.contains("x-goog-upload") {
        return Err(Category::UploadHeader);
    }
    if contains_armored_private_key(&lower) {
        return Err(Category::PrivateKey);
    }
    if lower.contains("/users/") || lower.contains("/home/") || lower.contains(":\\users\\") {
        return Err(Category::HomePath);
    }
    scan_urls(value)
}

fn contains_armored_private_key(value: &str) -> bool {
    let mut remaining = value;
    while let Some(start) = remaining.find("-----begin ") {
        let header = &remaining[start..];
        let marker = "-----begin ";
        let Some(end) = header[marker.len()..].find("-----") else {
            return false;
        };
        let end = marker.len() + end;
        if header[..end].contains("private key") {
            return true;
        }
        remaining = &header[marker.len()..];
    }
    false
}

fn scan_urls(value: &str) -> std::result::Result<(), Category> {
    let lower = value.to_ascii_lowercase();
    for scheme in ["https://", "http://"] {
        let mut offset = 0;
        while let Some(index) = lower[offset..].find(scheme) {
            let start = offset + index;
            let rest = &value[start..];
            let end = rest
                .find(|character: char| {
                    character.is_whitespace() || matches!(character, '\"' | '\'' | ')' | ']')
                })
                .unwrap_or(rest.len());
            let url = &rest[..end];
            let authority = url[scheme.len()..].split('/').next().unwrap_or_default();
            let url_lower = url.to_ascii_lowercase();
            if url.contains('?') || url.contains('#') || authority.contains('@') {
                return Err(Category::UrlCredential);
            }
            if authority.to_ascii_lowercase().contains("upload") || url_lower.contains("/upload/") {
                return Err(Category::UploadUrl);
            }
            offset = start + scheme.len();
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::{MAX_FILE_BYTES, read_bounded};

    #[test]
    fn bounded_reader_rejects_one_byte_over_the_cap_without_unbounded_allocation() {
        let input = vec![b'x'; usize::try_from(MAX_FILE_BYTES + 1).unwrap()];
        assert!(read_bounded(Cursor::new(input)).is_err());
    }
}
