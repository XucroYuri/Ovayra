use std::{
    collections::VecDeque,
    io::ErrorKind,
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    sync::{Arc, Mutex},
    thread,
    time::{Duration, Instant},
};

use spike_gemini::{FileState, GeminiClient, PollPolicy, RetryPolicy};
use spike_platform::{EnvelopeCipher, MemorySecretStore};
use wiremock::{Mock, MockServer, ResponseTemplate, matchers};

fn client(server: &MockServer) -> GeminiClient {
    GeminiClient::for_endpoints("api-key-that-must-not-leak", &server.uri(), &server.uri()).unwrap()
}

fn no_wait_client(server: &MockServer) -> GeminiClient {
    GeminiClient::for_endpoints_with_retry_policy(
        "api-key-that-must-not-leak",
        &server.uri(),
        &server.uri(),
        RetryPolicy::bounded(3, Duration::ZERO),
    )
    .unwrap()
}

const SERVER_IO_TIMEOUT: Duration = Duration::from_secs(1);
const SERVER_LIFETIME: Duration = Duration::from_secs(2);
type RecordedContractRequests = Arc<Mutex<Vec<(String, usize)>>>;

fn accept_before_deadline(listener: &TcpListener, deadline: Instant) -> Result<TcpStream, String> {
    loop {
        match listener.accept() {
            Ok((stream, _)) => {
                stream
                    .set_nonblocking(false)
                    .map_err(|error| format!("stream blocking setup: {error}"))?;
                return Ok(stream);
            }
            Err(error) if error.kind() == ErrorKind::WouldBlock => {
                if Instant::now() >= deadline {
                    return Err("accept timeout".to_owned());
                }
                thread::sleep(Duration::from_millis(1));
            }
            Err(error) => return Err(format!("accept error: {error}")),
        }
    }
}

fn read_request(stream: &mut TcpStream) -> Result<(String, usize), String> {
    stream
        .set_read_timeout(Some(SERVER_IO_TIMEOUT))
        .map_err(|error| format!("read timeout setup: {error}"))?;
    let mut bytes = Vec::new();
    let mut buffer = [0_u8; 4096];
    let header_end = loop {
        let count = stream
            .read(&mut buffer)
            .map_err(|error| format!("request header read: {error}"))?;
        if count == 0 {
            return Err("connection closed before request headers".to_owned());
        }
        bytes.extend_from_slice(&buffer[..count]);
        if let Some(position) = bytes.windows(4).position(|window| window == b"\r\n\r\n") {
            break position;
        }
    };
    let headers = String::from_utf8_lossy(&bytes[..header_end + 4]).into_owned();
    let content_length = headers
        .lines()
        .find_map(|line| {
            line.split_once(':').and_then(|(name, value)| {
                name.eq_ignore_ascii_case("content-length")
                    .then(|| value.trim().parse::<usize>().ok())
                    .flatten()
            })
        })
        .unwrap_or(0);
    while bytes.len() < header_end + 4 + content_length {
        let count = stream
            .read(&mut buffer)
            .map_err(|error| format!("request body read: {error}"))?;
        if count == 0 {
            return Err("connection closed before request body".to_owned());
        }
        bytes.extend_from_slice(&buffer[..count]);
    }
    Ok((headers, content_length))
}

fn write_response(stream: &mut TcpStream, response: &str) -> Result<(), String> {
    stream
        .set_write_timeout(Some(SERVER_IO_TIMEOUT))
        .map_err(|error| format!("write timeout setup: {error}"))?;
    stream
        .write_all(response.as_bytes())
        .map_err(|error| format!("response write: {error}"))?;
    stream
        .flush()
        .map_err(|error| format!("response flush: {error}"))
}

fn request_summary(headers: &str, body_length: usize) -> String {
    let request_line = headers.lines().next().unwrap_or("unknown");
    let command = header_value(headers, "x-goog-upload-command").unwrap_or("none");
    format!("{request_line} command={command} bytes={body_length}")
}

fn closing_upload_server(
    observed: u64,
) -> (String, Arc<Mutex<Vec<String>>>, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let address = listener.local_addr().unwrap();
    let requests = Arc::new(Mutex::new(Vec::new()));
    let recorded = Arc::clone(&requests);
    let task = thread::spawn(move || {
        let deadline = Instant::now() + SERVER_LIFETIME;
        for _ in 0..2 {
            let mut stream = match accept_before_deadline(&listener, deadline) {
                Ok(stream) => stream,
                Err(error) => {
                    recorded.lock().unwrap().push(error);
                    break;
                }
            };
            let (headers, body_length) = match read_request(&mut stream) {
                Ok(request) => request,
                Err(error) => {
                    recorded.lock().unwrap().push(error);
                    continue;
                }
            };
            let command = header_value(&headers, "x-goog-upload-command").unwrap_or_default();
            recorded
                .lock()
                .unwrap()
                .push(request_summary(&headers, body_length));
            if command == "query" {
                let reply = format!(
                    "HTTP/1.1 200 OK\r\nx-goog-upload-size-received: {observed}\r\ncontent-length: 0\r\nconnection: close\r\n\r\n"
                );
                if let Err(error) = write_response(&mut stream, &reply) {
                    recorded.lock().unwrap().push(error);
                }
            }
        }
    });
    (format!("http://{address}/session/1"), requests, task)
}

fn header_value<'a>(headers: &'a str, name: &str) -> Option<&'a str> {
    headers.lines().find_map(|line| {
        line.split_once(':').and_then(|(candidate, value)| {
            candidate.eq_ignore_ascii_case(name).then_some(value.trim())
        })
    })
}

fn resumable_contract_server() -> (String, RecordedContractRequests, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let address = listener.local_addr().unwrap();
    let requests = Arc::new(Mutex::new(Vec::new()));
    let recorded = Arc::clone(&requests);
    let session_url = format!("http://{address}/session/sensitive-upload-url");
    let task = thread::spawn(move || {
        let deadline = Instant::now() + SERVER_LIFETIME;
        for _ in 0..4 {
            let mut stream = match accept_before_deadline(&listener, deadline) {
                Ok(stream) => stream,
                Err(error) => {
                    recorded.lock().unwrap().push((error, 0));
                    break;
                }
            };
            let (headers, body_length) = match read_request(&mut stream) {
                Ok(request) => request,
                Err(error) => {
                    recorded.lock().unwrap().push((error, 0));
                    continue;
                }
            };
            let request_line = headers.lines().next().unwrap_or_default();
            let command = header_value(&headers, "x-goog-upload-command").unwrap_or_default();
            let offset = header_value(&headers, "x-goog-upload-offset");
            let label = if request_line.starts_with("POST /upload/v1beta/files ") {
                "start"
            } else if command == "upload" && offset == Some("0") {
                "upload"
            } else if command == "query" {
                "query"
            } else if command == "upload, finalize" && offset == Some("4") {
                "finalize"
            } else {
                "unexpected"
            };
            recorded
                .lock()
                .unwrap()
                .push((label.to_owned(), body_length));
            let response = match label {
                "start" => format!(
                    "HTTP/1.1 200 OK\r\nx-goog-upload-url: {session_url}\r\ncontent-length: 0\r\nconnection: close\r\n\r\n"
                ),
                "upload" | "query" => "HTTP/1.1 200 OK\r\nx-goog-upload-size-received: 4\r\ncontent-length: 0\r\nconnection: close\r\n\r\n".to_owned(),
                "finalize" => {
                    let body = r#"{"file":{"name":"files/1","uri":"gemini://files/1","mimeType":"video/webm","state":"PROCESSING"}}"#;
                    format!(
                        "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                        body.len()
                    )
                }
                _ => "HTTP/1.1 400 Bad Request\r\ncontent-length: 0\r\nconnection: close\r\n\r\n".to_owned(),
            };
            if let Err(error) = write_response(&mut stream, &response) {
                recorded.lock().unwrap().push((error, 0));
            }
        }
    });
    (format!("http://{address}"), requests, task)
}

#[tokio::test]
async fn starts_a_resumable_session_with_required_google_headers() {
    let server = MockServer::start().await;
    Mock::given(matchers::method("POST"))
        .and(matchers::path("/upload/v1beta/files"))
        .and(matchers::header("x-goog-upload-protocol", "resumable"))
        .and(matchers::header("x-goog-upload-command", "start"))
        .and(matchers::header("x-goog-upload-header-content-length", "8"))
        .and(matchers::header(
            "x-goog-upload-header-content-type",
            "video/webm",
        ))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("x-goog-upload-url", format!("{}/session/1", server.uri()))
                .insert_header("x-goog-upload-chunk-granularity", "4"),
        )
        .mount(&server)
        .await;

    let session = client(&server)
        .start_upload("synthetic", "video/webm", 8)
        .await
        .unwrap();
    assert_eq!(session.chunk_granularity(), Some(4));
    assert_eq!(format!("{session:?}"), "UploadSession([REDACTED])");
}

#[tokio::test]
async fn rejects_non_final_chunks_that_do_not_honor_server_granularity() {
    let server = MockServer::start().await;
    Mock::given(matchers::path("/upload/v1beta/files"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("x-goog-upload-url", format!("{}/session/1", server.uri()))
                .insert_header("x-goog-upload-chunk-granularity", "4"),
        )
        .mount(&server)
        .await;
    let client = no_wait_client(&server);
    let session = client
        .start_upload("synthetic", "video/webm", 8)
        .await
        .unwrap();
    assert!(client.upload_chunk(&session, 0, b"123").await.is_err());
}

#[tokio::test]
async fn chunk_retries_429_and_5xx_but_not_4xx() {
    for status in [429, 503] {
        let server = MockServer::start().await;
        Mock::given(matchers::path("/upload/v1beta/files"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("x-goog-upload-url", format!("{}/session/1", server.uri())),
            )
            .mount(&server)
            .await;
        let responses = Arc::new(Mutex::new(VecDeque::from([
            ResponseTemplate::new(status).insert_header("retry-after", "0"),
            ResponseTemplate::new(200),
        ])));
        Mock::given(matchers::header("x-goog-upload-command", "upload"))
            .respond_with({
                let responses = Arc::clone(&responses);
                move |_request: &wiremock::Request| {
                    responses
                        .lock()
                        .unwrap()
                        .pop_front()
                        .unwrap_or_else(|| ResponseTemplate::new(500))
                }
            })
            .mount(&server)
            .await;
        if status == 503 {
            Mock::given(matchers::header("x-goog-upload-command", "query"))
                .respond_with(
                    ResponseTemplate::new(200).insert_header("x-goog-upload-size-received", "0"),
                )
                .mount(&server)
                .await;
        }
        let client = no_wait_client(&server);
        let session = client
            .start_upload("synthetic", "video/webm", 4)
            .await
            .unwrap();
        let result = client.upload_chunk(&session, 0, b"1234").await;
        assert!(result.is_ok(), "status {status}: {result:?}");
        assert!(responses.lock().unwrap().is_empty());
    }
    let server = MockServer::start().await;
    Mock::given(matchers::path("/upload/v1beta/files"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("x-goog-upload-url", format!("{}/session/1", server.uri())),
        )
        .mount(&server)
        .await;
    Mock::given(matchers::header("x-goog-upload-command", "upload"))
        .respond_with(ResponseTemplate::new(400))
        .mount(&server)
        .await;
    let client = no_wait_client(&server);
    let session = client
        .start_upload("synthetic", "video/webm", 4)
        .await
        .unwrap();
    assert!(client.upload_chunk(&session, 0, b"1234").await.is_err());
}

#[tokio::test]
async fn ambiguous_chunk_transport_queries_and_accepts_exact_expected_offset() {
    let (url, requests, task) = closing_upload_server(4);
    let server = MockServer::start().await;
    Mock::given(matchers::path("/upload/v1beta/files"))
        .respond_with(ResponseTemplate::new(200).insert_header("x-goog-upload-url", url))
        .mount(&server)
        .await;
    let client = no_wait_client(&server);
    let session = client
        .start_upload("synthetic", "video/webm", 4)
        .await
        .unwrap();
    let result = client.upload_chunk(&session, 0, b"1234").await;
    task.join().unwrap();
    let requests = requests.lock().unwrap();
    assert!(result.is_ok(), "{result:?} {requests:?}");
    assert_eq!(requests.len(), 2);
    assert!(requests[0].contains("POST /session/1"));
    assert!(requests[0].contains("command=upload"));
    assert!(requests[1].contains("POST /session/1"));
    assert!(requests[1].contains("command=query"));
}

#[tokio::test]
async fn ambiguous_chunk_lower_observed_offset_fails_without_stale_replay() {
    let (url, requests, task) = closing_upload_server(2);
    let server = MockServer::start().await;
    Mock::given(matchers::path("/upload/v1beta/files"))
        .respond_with(ResponseTemplate::new(200).insert_header("x-goog-upload-url", url))
        .mount(&server)
        .await;
    let client = no_wait_client(&server);
    let session = client
        .start_upload("synthetic", "video/webm", 4)
        .await
        .unwrap();
    let result = client.upload_chunk(&session, 0, b"1234").await;
    task.join().unwrap();
    let requests = requests.lock().unwrap();
    assert!(result.is_err(), "{result:?} {requests:?}");
    assert_eq!(requests.len(), 2);
    assert!(requests[0].contains("command=upload"));
    assert!(requests[1].contains("command=query"));
}

#[tokio::test]
async fn ambiguous_chunk_higher_observed_offset_fails_without_stale_replay() {
    let (url, requests, task) = closing_upload_server(8);
    let server = MockServer::start().await;
    Mock::given(matchers::path("/upload/v1beta/files"))
        .respond_with(ResponseTemplate::new(200).insert_header("x-goog-upload-url", url))
        .mount(&server)
        .await;
    let client = no_wait_client(&server);
    let session = client
        .start_upload("synthetic", "video/webm", 4)
        .await
        .unwrap();
    let result = client.upload_chunk(&session, 0, b"1234").await;
    task.join().unwrap();
    let requests = requests.lock().unwrap();
    assert!(result.is_err(), "{result:?} {requests:?}");
    assert_eq!(requests.len(), 2);
    assert!(requests[0].contains("command=upload"));
    assert!(requests[1].contains("command=query"));
}

#[tokio::test]
async fn sends_chunks_queries_finalizes_and_never_leaks_sensitive_values() {
    let (server_url, requests, task) = resumable_contract_server();
    let client = GeminiClient::for_endpoints_with_retry_policy(
        "api-key-that-must-not-leak",
        &server_url,
        &server_url,
        RetryPolicy::bounded(3, Duration::ZERO),
    )
    .unwrap();
    let session = client
        .start_upload("synthetic", "video/webm", 8)
        .await
        .unwrap();
    let cipher =
        EnvelopeCipher::load_or_create(&MemorySecretStore::default(), "gemini-contract").unwrap();
    let checkpoint = client.checkpoint(&cipher, &session, 0).unwrap();
    let checkpoint_json = serde_json::to_string(&checkpoint).unwrap();
    assert!(!checkpoint_json.contains("sensitive-upload-url"));
    let resumed = client.resume_checkpoint(&cipher, &checkpoint).unwrap();
    assert_eq!(resumed.staged_offset(), 0);
    assert_eq!(format!("{resumed:?}"), "ResumedUpload([REDACTED])");
    client.upload_chunk(&session, 0, b"1234").await.unwrap();
    assert_eq!(client.query_offset(&session).await.unwrap(), 4);
    let remote = client.finalize_chunk(&session, 4, b"5678").await.unwrap();
    task.join().unwrap();
    assert_eq!(
        *requests.lock().unwrap(),
        vec![
            ("start".to_owned(), 36),
            ("upload".to_owned(), 4),
            ("query".to_owned(), 0),
            ("finalize".to_owned(), 4),
        ]
    );
    assert_eq!(remote.state, FileState::Processing);
    let text = format!("{session:?} {client:?}");
    assert!(!text.contains("api-key-that-must-not-leak"));
    assert!(!text.contains("sensitive-upload-url"));
}

#[tokio::test]
async fn polls_generates_and_deletes_with_complete_shapes() {
    let server = MockServer::start().await;
    Mock::given(matchers::method("GET"))
        .and(matchers::path("/v1beta/files/1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
          "name":"files/1", "uri":"gemini://files/1", "mimeType":"video/webm", "state":"PROCESSING", "sizeBytes":"8", "createTime":"2026-07-17T00:00:00Z", "updateTime":"2026-07-17T00:00:01Z", "expirationTime":"2026-07-19T00:00:00Z", "sha256Hash":"abc", "displayName":"synthetic", "videoMetadata": {"videoDuration":"8s"}
        })))
        .up_to_n_times(1)
        .mount(&server)
        .await;
    Mock::given(matchers::method("GET")).and(matchers::path("/v1beta/files/1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
          "name":"files/1", "uri":"gemini://files/1", "mimeType":"video/webm", "state":"ACTIVE", "sizeBytes":"8", "createTime":"2026-07-17T00:00:00Z", "updateTime":"2026-07-17T00:00:02Z", "expirationTime":"2026-07-19T00:00:00Z", "sha256Hash":"abc", "displayName":"synthetic", "videoMetadata": {"videoDuration":"8s"}
        }))).mount(&server).await;
    Mock::given(matchers::method("POST")).and(matchers::path("/v1beta/models/gemini-3.1-flash-lite:generateContent"))
        .and(matchers::body_json(serde_json::json!({"contents":[{"role":"user","parts":[{"fileData":{"fileUri":"gemini://files/1","mimeType":"video/webm"}},{"text":"Describe the synthetic test video in one sentence."}]}]})))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
          "candidates":[{"content":{"role":"model","parts":[{"text":"A synthetic video."}]},"finishReason":"STOP","index":0,"safetyRatings":[]}], "usageMetadata":{"promptTokenCount":1,"candidatesTokenCount":1,"totalTokenCount":2}, "modelVersion":"gemini-3.1-flash-lite", "responseId":"redacted-id"
        }))).mount(&server).await;
    Mock::given(matchers::method("DELETE"))
        .and(matchers::path("/v1beta/files/1"))
        .respond_with(ResponseTemplate::new(204))
        .mount(&server)
        .await;
    let client = client(&server);
    let remote = client
        .poll_until_ready("files/1", Duration::from_millis(1), Duration::from_secs(1))
        .await
        .unwrap();
    assert_eq!(remote.state, FileState::Active);
    let generation = client
        .generate_content(&remote, "gemini-3.1-flash-lite")
        .await
        .unwrap();
    assert!(generation.analysis_nonempty());
    assert_eq!(generation.status(), 200);
    assert_eq!(generation.model(), "gemini-3.1-flash-lite");
    assert!(generation.response_bytes() > 0);
    assert!(!format!("{generation:?}").contains("synthetic video"));
    client.delete_file(&remote.name).await.unwrap();
}

#[tokio::test]
async fn retries_transient_errors_honors_retry_after_and_does_not_retry_client_errors() {
    let transient = MockServer::start().await;
    Mock::given(matchers::path("/upload/v1beta/files"))
        .respond_with(ResponseTemplate::new(429).insert_header("retry-after", "0"))
        .up_to_n_times(1)
        .mount(&transient)
        .await;
    Mock::given(matchers::path("/upload/v1beta/files"))
        .respond_with(ResponseTemplate::new(200).insert_header(
            "x-goog-upload-url",
            format!("{}/session/1", transient.uri()),
        ))
        .mount(&transient)
        .await;
    assert!(
        client(&transient)
            .start_upload("synthetic", "video/webm", 1)
            .await
            .is_ok()
    );

    let server_error = MockServer::start().await;
    Mock::given(matchers::path("/upload/v1beta/files"))
        .respond_with(ResponseTemplate::new(503))
        .up_to_n_times(1)
        .mount(&server_error)
        .await;
    Mock::given(matchers::path("/upload/v1beta/files"))
        .respond_with(ResponseTemplate::new(200).insert_header(
            "x-goog-upload-url",
            format!("{}/session/1", server_error.uri()),
        ))
        .mount(&server_error)
        .await;
    assert!(
        client(&server_error)
            .start_upload("synthetic", "video/webm", 1)
            .await
            .is_ok()
    );

    for status in [400, 401, 403] {
        let permanent = MockServer::start().await;
        Mock::given(matchers::path("/upload/v1beta/files"))
            .respond_with(ResponseTemplate::new(status))
            .up_to_n_times(1)
            .mount(&permanent)
            .await;
        Mock::given(matchers::path("/upload/v1beta/files"))
            .respond_with(ResponseTemplate::new(200).insert_header(
                "x-goog-upload-url",
                format!("{}/session/1", permanent.uri()),
            ))
            .mount(&permanent)
            .await;
        assert!(
            client(&permanent)
                .start_upload("synthetic", "video/webm", 1)
                .await
                .is_err()
        );
    }
}

#[tokio::test]
async fn caps_nonzero_retry_after_and_never_blindly_replays_transport_errors() {
    let server = MockServer::start().await;
    Mock::given(matchers::path("/upload/v1beta/files"))
        .respond_with(ResponseTemplate::new(429).insert_header("retry-after", "3600"))
        .up_to_n_times(1)
        .mount(&server)
        .await;
    Mock::given(matchers::path("/upload/v1beta/files"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("x-goog-upload-url", format!("{}/session/1", server.uri())),
        )
        .mount(&server)
        .await;
    assert!(
        no_wait_client(&server)
            .start_upload("synthetic", "video/webm", 1)
            .await
            .is_ok()
    );

    let transport = GeminiClient::for_endpoints_with_retry_policy(
        "api-key-that-must-not-leak",
        "http://127.0.0.1:1",
        "http://127.0.0.1:1",
        RetryPolicy::bounded(3, Duration::from_secs(1)),
    )
    .unwrap();
    assert!(
        transport
            .start_upload("synthetic", "video/webm", 1)
            .await
            .is_err()
    );
}

#[tokio::test]
async fn persistent_processing_returns_poll_timeout() {
    let server = MockServer::start().await;
    Mock::given(matchers::method("GET")).and(matchers::path("/v1beta/files/processing"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"name":"files/processing","uri":"gemini://files/processing","mimeType":"video/webm","state":"PROCESSING","sizeBytes":"8","createTime":"2026-07-17T00:00:00Z","updateTime":"2026-07-17T00:00:01Z","expirationTime":"2026-07-19T00:00:00Z","sha256Hash":"abc","displayName":"synthetic","videoMetadata":{"videoDuration":"8s"}})))
        .mount(&server).await;
    for _ in 0..20 {
        let error = client(&server)
            .poll_until_ready_with_policy(
                "files/processing",
                PollPolicy::bounded(Duration::ZERO, Duration::ZERO),
            )
            .await
            .unwrap_err();
        assert!(matches!(error, spike_gemini::GeminiError::PollTimeout));
    }
}

#[tokio::test]
async fn decoded_empty_generation_returns_redacted_failure_metrics() {
    let server = MockServer::start().await;
    Mock::given(matchers::method("POST"))
        .and(matchers::path("/v1beta/models/gemini-3.1-flash-lite:generateContent"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"candidates":[{"content":{"role":"model","parts":[{"text":""}]},"finishReason":"STOP","index":0,"safetyRatings":[]}],"usageMetadata":{"promptTokenCount":1,"candidatesTokenCount":1,"totalTokenCount":2},"modelVersion":"gemini-3.1-flash-lite","responseId":"redacted"})))
        .mount(&server).await;
    let remote = spike_gemini::RemoteFile {
        name: "files/1".to_owned(),
        uri: "gemini://files/1".to_owned(),
        mime_type: "video/webm".to_owned(),
        state: FileState::Active,
    };
    let result = client(&server)
        .generate_content(&remote, "gemini-3.1-flash-lite")
        .await
        .unwrap();
    assert!(!result.analysis_nonempty());
    assert_eq!(result.status(), 200);
    assert!(result.response_bytes() > 0);
}
