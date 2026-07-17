use std::{
    collections::BTreeSet,
    fmt, fs,
    path::{Path, PathBuf},
};

use anyhow::Result;
use serde::de::{self, Error as _, MapAccess, SeqAccess, Visitor};
use serde::{Deserialize, Deserializer};
use spike_contracts::{Evidence, SpikeId, Verdict};

const MAX_FILE_BYTES: u64 = 1_048_576;
const MAX_FILES: usize = 1_024;
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
            Self::Io => "io",
        }
    }
}

struct LintError {
    relative: PathBuf,
    category: Category,
}

impl LintError {
    fn new(relative: impl Into<PathBuf>, category: Category) -> Self {
        Self {
            relative: relative.into(),
            category,
        }
    }
}

impl fmt::Display for LintError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "evidence lint rejected {}: {}",
            self.relative.display(),
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

/// Scans only bounded regular files below `directory`, never follows symlinks,
/// and reports a stable relative-path/category diagnostic on rejection.
pub(crate) fn lint_dir(directory: &Path, text_mode: bool) -> Result<()> {
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

    let mut files = Vec::new();
    collect_regular_files(directory, directory, &mut files)?;
    if files.len() > MAX_FILES {
        return Err(LintError::new(".", Category::TooManyFiles).into());
    }
    for file in files {
        lint_file(directory, &file, text_mode)?;
    }
    println!("EVIDENCE_LINT=PASS");
    Ok(())
}

/// Validates the documented desktop preview threshold from parsed `Evidence`,
/// never from a human-oriented console line.
pub(crate) fn verify_preview(file: &Path) -> Result<()> {
    let contents = fs::read_to_string(file).map_err(|_| LintError::new(".", Category::Io))?;
    let evidence = Evidence::from_json(&contents)
        .map_err(|_| LintError::new(".", Category::InvalidEvidence))?;
    if evidence.spike() != SpikeId::Preview || evidence.verdict() != Some(Verdict::Pass) {
        anyhow::bail!("preview evidence did not report pass");
    }
    let measurements = evidence.measurements();
    let observed_milli_fps = measurement_u64(measurements, "observed_milli_fps")?;
    let requested_duration_seconds = measurement_u64(measurements, "requested_duration_seconds")?;
    let measured_elapsed_ms = measurement_u64(measurements, "measured_elapsed_ms")?;
    let p95_ms = measurement_u64(measurements, "p95_ms")?;
    let rss_growth_mib = measurement_u64(measurements, "rss_growth_mib")?;
    let hidden = measurement_bool(measurements, "automation_hide")?;
    let restored = measurement_bool(measurements, "automation_restore")?;
    let samples_complete = measurement_bool(measurements, "rss_samples_complete")?;
    if !(23_000..=25_000).contains(&observed_milli_fps)
        || requested_duration_seconds != 120
        || measured_elapsed_ms < 120_000
        || evidence.duration_ms().unwrap_or(0) < 120_000
        || !hidden
        || !restored
        || p95_ms > 100
        || rss_growth_mib > 64
        || !samples_complete
    {
        anyhow::bail!("preview evidence did not meet the documented threshold");
    }
    println!("PREVIEW_EVIDENCE=PASS");
    Ok(())
}

fn measurement_u64(
    measurements: &std::collections::BTreeMap<String, serde_json::Value>,
    name: &str,
) -> Result<u64> {
    measurements
        .get(name)
        .and_then(serde_json::Value::as_u64)
        .ok_or_else(|| {
            anyhow::anyhow!("preview evidence is missing a required numeric measurement")
        })
}

fn measurement_bool(
    measurements: &std::collections::BTreeMap<String, serde_json::Value>,
    name: &str,
) -> Result<bool> {
    measurements
        .get(name)
        .and_then(serde_json::Value::as_bool)
        .ok_or_else(|| {
            anyhow::anyhow!("preview evidence is missing a required boolean measurement")
        })
}

fn collect_regular_files(root: &Path, current: &Path, files: &mut Vec<PathBuf>) -> Result<()> {
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
            collect_regular_files(root, &path, files)?;
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

fn lint_file(root: &Path, file: &Path, text_mode: bool) -> Result<()> {
    let relative = relative(root, file);
    if !text_mode && relative == Path::new(".gitkeep") {
        return Ok(());
    }
    let metadata = fs::metadata(file).map_err(|_| LintError::new(&relative, Category::Io))?;
    if metadata.len() > MAX_FILE_BYTES {
        return Err(LintError::new(relative, Category::Oversized).into());
    }
    if !text_mode && file.extension().and_then(|extension| extension.to_str()) != Some("json") {
        return Err(LintError::new(relative, Category::UnsupportedFile).into());
    }
    let bytes = fs::read(file).map_err(|_| LintError::new(&relative, Category::Io))?;
    if bytes.contains(&0) {
        return Err(LintError::new(relative, Category::BinaryOrInvalidText).into());
    }
    let contents = std::str::from_utf8(&bytes)
        .map_err(|_| LintError::new(&relative, Category::BinaryOrInvalidText))?;
    if text_mode {
        scan_text(contents).map_err(|category| LintError::new(relative, category))?;
        return Ok(());
    }
    parse_and_scan_json(contents).map_err(|category| LintError::new(&relative, category))?;
    Evidence::from_json(contents)
        .map_err(|_| LintError::new(relative, Category::InvalidEvidence))?;
    Ok(())
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
    let normalized: String = key
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .flat_map(char::to_lowercase)
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
    if lower.contains("-----begin") && lower.contains("private key-----") {
        return Err(Category::PrivateKey);
    }
    if lower.contains("/users/") || lower.contains("/home/") || lower.contains(":\\users\\") {
        return Err(Category::HomePath);
    }
    scan_urls(value)
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
