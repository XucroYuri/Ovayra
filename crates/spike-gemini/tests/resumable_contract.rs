use std::time::Duration;

use spike_gemini::{FileState, GeminiClient, RetryPolicy};
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
    let client = client(&server);
    let session = client
        .start_upload("synthetic", "video/webm", 8)
        .await
        .unwrap();
    assert!(client.upload_chunk(&session, 0, b"123").await.is_err());
}

#[tokio::test]
async fn sends_chunks_queries_finalizes_and_never_leaks_sensitive_values() {
    let server = MockServer::start().await;
    let upload_url = format!("{}/session/sensitive-upload-url", server.uri());
    Mock::given(matchers::path("/upload/v1beta/files"))
        .respond_with(ResponseTemplate::new(200).insert_header("x-goog-upload-url", &upload_url))
        .mount(&server)
        .await;
    Mock::given(matchers::method("POST"))
        .and(matchers::path("/session/sensitive-upload-url"))
        .and(matchers::header("x-goog-upload-command", "upload"))
        .and(matchers::header("x-goog-upload-offset", "0"))
        .respond_with(ResponseTemplate::new(200).insert_header("x-goog-upload-size-received", "4"))
        .up_to_n_times(1)
        .mount(&server)
        .await;
    Mock::given(matchers::method("POST"))
        .and(matchers::path("/session/sensitive-upload-url"))
        .and(matchers::header("x-goog-upload-command", "query"))
        .respond_with(ResponseTemplate::new(200).insert_header("x-goog-upload-size-received", "4"))
        .mount(&server)
        .await;
    Mock::given(matchers::method("POST"))
        .and(matchers::path("/session/sensitive-upload-url"))
        .and(matchers::headers("x-goog-upload-command", vec!["upload", "finalize"]))
        .and(matchers::header("x-goog-upload-offset", "4"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "file": {"name":"files/1", "uri":"gemini://files/1", "mimeType":"video/webm", "state":"PROCESSING"}
        })))
        .mount(&server).await;
    let client = client(&server);
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
async fn polling_times_out_and_failed_files_are_terminal() {
    let server = MockServer::start().await;
    Mock::given(matchers::method("GET")).and(matchers::path("/v1beta/files/failed"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"name":"files/failed","uri":"gemini://files/failed","mimeType":"video/webm","state":"FAILED","error":{"code":13,"message":"remote failure","status":"INTERNAL"}})))
        .mount(&server).await;
    let error = client(&server)
        .poll_until_ready(
            "files/failed",
            Duration::from_millis(1),
            Duration::from_millis(5),
        )
        .await
        .unwrap_err();
    assert!(!format!("{error:?}").contains("api-key-that-must-not-leak"));
}
