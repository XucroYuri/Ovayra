use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
pub(crate) struct UploadResponse {
    pub file: FileDto,
}

#[derive(Debug, Deserialize)]
pub(crate) struct FileDto {
    pub name: String,
    pub uri: String,
    #[serde(rename = "mimeType")]
    pub mime_type: String,
    pub state: Option<String>,
}

#[derive(Debug, Serialize)]
pub(crate) struct StartUploadRequest<'a> {
    pub file: StartFile<'a>,
}

#[derive(Debug, Serialize)]
pub(crate) struct StartFile<'a> {
    #[serde(rename = "displayName")]
    pub display_name: &'a str,
}

#[derive(Debug, Serialize)]
pub(crate) struct GenerateRequest<'a> {
    pub contents: [GenerateContent<'a>; 1],
}

#[derive(Debug, Serialize)]
pub(crate) struct GenerateContent<'a> {
    pub role: &'static str,
    pub parts: [GeneratePart<'a>; 2],
}

#[derive(Debug, Serialize)]
#[serde(untagged)]
pub(crate) enum GeneratePart<'a> {
    FileData {
        #[serde(rename = "fileData")]
        file_data: FileData<'a>,
    },
    Text {
        text: &'a str,
    },
}

#[derive(Debug, Serialize)]
pub(crate) struct FileData<'a> {
    #[serde(rename = "fileUri")]
    pub file_uri: &'a str,
    #[serde(rename = "mimeType")]
    pub mime_type: &'a str,
}

#[derive(Debug, Deserialize)]
pub(crate) struct GenerateResponse {
    pub candidates: Vec<Candidate>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct Candidate {
    pub content: GeneratedContent,
}

#[derive(Debug, Deserialize)]
pub(crate) struct GeneratedContent {
    pub parts: Vec<GeneratedPart>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct GeneratedPart {
    pub text: Option<String>,
}
