use std::{path::Path, sync::Arc, time::Duration};

use reqwest::{
    multipart::{Form, Part},
    Client, RequestBuilder, StatusCode,
};
use serde::{de::DeserializeOwned, Deserialize};
use serde_json::{json, Value};

use super::{
    normalize::normalize_jsonl, InputDoc, OcrService, ParseCheckpoint, ParseOptions, ProgressFn,
};
use crate::model::{OcrError, RecognitionResult, ServiceId};

const BASE_URL: &str = "https://paddleocr.aistudio-app.com";
const JOBS_PATH: &str = "/api/v2/ocr/jobs";
const SEGMENT_SIZE: usize = 100;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProxyConfig {
    System,
    Custom(String),
    Direct,
}

pub type TokenProvider = Arc<dyn Fn() -> Result<String, OcrError> + Send + Sync>;
pub type ProxyProvider = Arc<dyn Fn() -> Result<ProxyConfig, OcrError> + Send + Sync>;

pub struct PaddleOcr {
    service: ServiceId,
    base_url: String,
    token: TokenProvider,
    proxy: ProxyProvider,
    poll_interval: Duration,
}

impl PaddleOcr {
    pub fn new(service: ServiceId, token: TokenProvider, proxy: ProxyProvider) -> Self {
        Self {
            service,
            base_url: BASE_URL.into(),
            token,
            proxy,
            poll_interval: Duration::from_secs(5),
        }
    }

    #[cfg(test)]
    fn with_endpoint(
        service: ServiceId,
        token: TokenProvider,
        proxy: ProxyProvider,
        base_url: String,
        poll_interval: Duration,
    ) -> Self {
        Self {
            service,
            base_url,
            token,
            proxy,
            poll_interval,
        }
    }

    pub async fn probe_token(
        base_url: &str,
        token: &str,
        proxy: ProxyConfig,
    ) -> Result<bool, OcrError> {
        if token.trim().is_empty() {
            return Ok(false);
        }
        let client = build_client(proxy)?;
        let url = format!(
            "{}{JOBS_PATH}/paddledesk-token-validation",
            base_url.trim_end_matches('/')
        );
        match request_data::<Value>(client.get(url).bearer_auth(token), token).await {
            Ok(_) => Ok(true),
            Err(failure) if failure.code == Some(11001) => Ok(true),
            Err(ApiFailure {
                error: OcrError::Auth,
                ..
            }) => Ok(false),
            Err(failure) => Err(failure.error),
        }
    }

    async fn parse_inner(
        &self,
        input: &InputDoc,
        progress: ProgressFn,
        checkpoint: ParseCheckpoint,
    ) -> Result<RecognitionResult, OcrError> {
        let token = (self.token)()?;
        if token.trim().is_empty() {
            return Err(OcrError::Auth);
        }
        let client = build_client((self.proxy)()?)?;
        let segments = segments(&input.path)?;
        let total_pages = segments.iter().map(|segment| segment.pages).sum();
        let mut job_ids = checkpoint.job_ids()?;
        let mut completed_pages = 0;
        let mut aggregate = RecognitionResult {
            markdown: String::new(),
            page_count: 0,
            pages: Vec::new(),
        };

        for (index, segment) in segments.iter().enumerate() {
            let mut job_id = job_ids.get(index).filter(|id| !id.is_empty()).cloned();
            if job_id.is_none() {
                let submitted = self
                    .submit(&client, &token, &input.path, segment.range.as_deref())
                    .await?;
                save_job_id(&checkpoint, &mut job_ids, index, submitted.clone())?;
                job_id = Some(submitted);
            }

            let json_url = loop {
                let id = job_id.as_deref().expect("job id set before polling");
                match self
                    .poll_until_done(&client, &token, id, completed_pages, total_pages, &progress)
                    .await
                {
                    Ok(url) => break url,
                    Err(PollError::Expired) => {
                        let replacement = self
                            .submit(&client, &token, &input.path, segment.range.as_deref())
                            .await?;
                        save_job_id(&checkpoint, &mut job_ids, index, replacement.clone())?;
                        job_id = Some(replacement);
                    }
                    Err(PollError::Ocr(error)) => return Err(error),
                }
            };

            let body = download_jsonl(&client, &json_url).await?;
            append_result(&mut aggregate, normalize_jsonl(self.service, &body)?);
            completed_pages += segment.pages;
            progress(completed_pages, total_pages);
        }

        aggregate.page_count = aggregate.pages.len() as u32;
        Ok(aggregate)
    }

    async fn submit(
        &self,
        client: &Client,
        token: &str,
        path: &Path,
        page_range: Option<&str>,
    ) -> Result<String, OcrError> {
        let bytes = std::fs::read(path)
            .map_err(|error| OcrError::InvalidInput(format!("cannot read input: {error}")))?;
        let file_name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("upload.bin")
            .to_string();
        let mut form = Form::new()
            .part("file", Part::bytes(bytes).file_name(file_name))
            .text("model", model_name(self.service))
            .text(
                "optionalPayload",
                optional_payload(self.service).to_string(),
            );
        if let Some(range) = page_range {
            form = form.text("pageRanges", range.to_string());
        }
        let url = format!("{}{JOBS_PATH}", self.base_url.trim_end_matches('/'));
        let data: SubmitData =
            request_data(client.post(url).bearer_auth(token).multipart(form), token)
                .await
                .map_err(|failure| failure.error)?;
        Ok(data.job_id)
    }

    async fn poll_until_done(
        &self,
        client: &Client,
        token: &str,
        job_id: &str,
        completed_pages: u32,
        total_pages: u32,
        progress: &ProgressFn,
    ) -> Result<String, PollError> {
        let url = format!(
            "{}{JOBS_PATH}/{job_id}",
            self.base_url.trim_end_matches('/')
        );
        loop {
            let data: PollData =
                match request_data(client.get(&url).bearer_auth(token), token).await {
                    Ok(data) => data,
                    Err(failure) if failure.code == Some(11002) => return Err(PollError::Expired),
                    Err(failure) => return Err(PollError::Ocr(failure.error)),
                };
            match data.state.as_str() {
                "pending" => {}
                "running" => {
                    if let Some(extracted) = data
                        .extract_progress
                        .as_ref()
                        .and_then(|value| number(&value.extracted_pages))
                    {
                        progress(completed_pages.saturating_add(extracted), total_pages);
                    }
                }
                "done" => {
                    return data.result_url.map(|urls| urls.json_url).ok_or_else(|| {
                        PollError::Ocr(OcrError::Parse(
                            "completed job is missing resultUrl.jsonUrl".into(),
                        ))
                    });
                }
                "failed" => {
                    return Err(PollError::Ocr(OcrError::Parse(
                        data.error_msg
                            .unwrap_or_else(|| "upstream OCR job failed".into()),
                    )));
                }
                state => {
                    return Err(PollError::Ocr(OcrError::Parse(format!(
                        "unknown upstream job state: {state}"
                    ))));
                }
            }
            tokio::time::sleep(self.poll_interval).await;
        }
    }
}

#[async_trait::async_trait]
impl OcrService for PaddleOcr {
    fn id(&self) -> ServiceId {
        self.service
    }

    async fn parse(
        &self,
        input: &InputDoc,
        _opts: &ParseOptions,
        progress: ProgressFn,
    ) -> Result<RecognitionResult, OcrError> {
        self.parse_inner(input, progress, ParseCheckpoint::empty())
            .await
    }

    async fn parse_resumable(
        &self,
        input: &InputDoc,
        _opts: &ParseOptions,
        progress: ProgressFn,
        checkpoint: ParseCheckpoint,
    ) -> Result<RecognitionResult, OcrError> {
        self.parse_inner(input, progress, checkpoint).await
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SubmitData {
    job_id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PollData {
    state: String,
    #[serde(default)]
    extract_progress: Option<ExtractProgress>,
    #[serde(default)]
    result_url: Option<ResultUrl>,
    #[serde(default)]
    error_msg: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ExtractProgress {
    extracted_pages: Value,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ResultUrl {
    json_url: String,
}

struct ApiFailure {
    code: Option<i64>,
    error: OcrError,
}

enum PollError {
    Expired,
    Ocr(OcrError),
}

#[derive(Clone)]
struct Segment {
    range: Option<String>,
    pages: u32,
}

async fn request_data<T: DeserializeOwned>(
    request: RequestBuilder,
    token: &str,
) -> Result<T, ApiFailure> {
    let response = request.send().await.map_err(|error| ApiFailure {
        code: None,
        error: OcrError::Network(redact(error.to_string(), token)),
    })?;
    let status = response.status();
    let body = response.text().await.map_err(|error| ApiFailure {
        code: None,
        error: OcrError::Network(redact(error.to_string(), token)),
    })?;
    let value: Value = match serde_json::from_str(&body) {
        Ok(value) => value,
        Err(error) if status.is_success() => {
            return Err(ApiFailure {
                code: None,
                error: OcrError::Parse(format!("invalid API response: {error}")),
            });
        }
        Err(_) => {
            return Err(ApiFailure {
                code: None,
                error: map_failure(status, None, redact(body, token)),
            });
        }
    };
    let code = integer(value.get("code"));
    if !status.is_success() || code.is_some_and(|code| code != 0) {
        let detail = response_detail(&value)
            .map(|detail| redact(detail, token))
            .unwrap_or_else(|| redact(body, token));
        return Err(ApiFailure {
            code,
            error: map_failure(status, code, detail),
        });
    }
    let data = value.get("data").cloned().ok_or_else(|| ApiFailure {
        code,
        error: OcrError::Parse("API response is missing data".into()),
    })?;
    serde_json::from_value(data).map_err(|error| ApiFailure {
        code,
        error: OcrError::Parse(format!("invalid API data: {error}")),
    })
}

async fn download_jsonl(client: &Client, url: &str) -> Result<String, OcrError> {
    let response = client
        .get(url)
        .send()
        .await
        .map_err(|error| OcrError::Network(error.to_string()))?;
    let status = response.status();
    let body = response
        .text()
        .await
        .map_err(|error| OcrError::Network(error.to_string()))?;
    if status.is_success() {
        Ok(body)
    } else {
        Err(map_failure(status, None, body))
    }
}

fn build_client(proxy: ProxyConfig) -> Result<Client, OcrError> {
    let builder = Client::builder();
    let builder = match proxy {
        ProxyConfig::System => builder,
        ProxyConfig::Custom(url) => builder.proxy(
            reqwest::Proxy::all(&url)
                .map_err(|error| OcrError::InvalidInput(format!("invalid proxy URL: {error}")))?,
        ),
        ProxyConfig::Direct => builder.no_proxy(),
    };
    builder
        .build()
        .map_err(|error| OcrError::Network(format!("HTTP client setup failed: {error}")))
}

fn segments(path: &Path) -> Result<Vec<Segment>, OcrError> {
    let is_pdf = path
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("pdf"));
    if !is_pdf {
        return Ok(vec![Segment {
            range: None,
            pages: 1,
        }]);
    }
    let document = lopdf::Document::load(path)
        .map_err(|error| OcrError::InvalidInput(format!("cannot read PDF pages: {error}")))?;
    let page_count = document.get_pages().len();
    if page_count == 0 {
        return Err(OcrError::InvalidInput("PDF contains no pages".into()));
    }
    Ok(page_ranges(page_count)
        .into_iter()
        .enumerate()
        .map(|(index, range)| Segment {
            range: Some(range),
            pages: (page_count - index * SEGMENT_SIZE).min(SEGMENT_SIZE) as u32,
        })
        .collect())
}

fn page_ranges(page_count: usize) -> Vec<String> {
    (1..=page_count)
        .step_by(SEGMENT_SIZE)
        .map(|start| {
            let end = (start + SEGMENT_SIZE - 1).min(page_count);
            if start == end {
                start.to_string()
            } else {
                format!("{start}-{end}")
            }
        })
        .collect()
}

fn save_job_id(
    checkpoint: &ParseCheckpoint,
    job_ids: &mut Vec<String>,
    index: usize,
    job_id: String,
) -> Result<(), OcrError> {
    if job_ids.len() <= index {
        job_ids.resize(index + 1, String::new());
    }
    job_ids[index] = job_id;
    checkpoint.save(job_ids.clone())
}

fn append_result(aggregate: &mut RecognitionResult, mut result: RecognitionResult) {
    if !result.markdown.is_empty() {
        if !aggregate.markdown.is_empty() {
            aggregate.markdown.push_str("\n\n");
        }
        aggregate.markdown.push_str(&result.markdown);
    }
    aggregate.pages.append(&mut result.pages);
}

fn model_name(service: ServiceId) -> &'static str {
    match service {
        ServiceId::Vl16 => "PaddleOCR-VL-1.6",
        ServiceId::PpOcrV6 => "PP-OCRv6",
        ServiceId::StructureV3 => "PP-StructureV3",
    }
}

fn optional_payload(service: ServiceId) -> Value {
    match service {
        ServiceId::PpOcrV6 => json!({
            "useDocOrientationClassify": false,
            "useDocUnwarping": false,
            "useTextlineOrientation": false
        }),
        ServiceId::Vl16 | ServiceId::StructureV3 => json!({
            "useDocOrientationClassify": false,
            "useDocUnwarping": false,
            "useChartRecognition": false
        }),
    }
}

fn response_detail(value: &Value) -> Option<String> {
    value
        .pointer("/data/errorMsg")
        .and_then(Value::as_str)
        .or_else(|| value.get("msg").and_then(Value::as_str))
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn integer(value: Option<&Value>) -> Option<i64> {
    value.and_then(|value| {
        value
            .as_i64()
            .or_else(|| value.as_str().and_then(|value| value.parse().ok()))
    })
}

fn number(value: &Value) -> Option<u32> {
    value
        .as_u64()
        .and_then(|value| u32::try_from(value).ok())
        .or_else(|| value.as_str().and_then(|value| value.parse().ok()))
}

fn map_failure(status: StatusCode, code: Option<i64>, detail: String) -> OcrError {
    match code {
        Some(12001) => OcrError::Quota,
        Some(12002 | 10010) => OcrError::RateLimited(detail),
        Some(10001..=10009) => OcrError::InvalidInput(detail),
        Some(11001..=11003) => OcrError::Parse(detail),
        _ if status == StatusCode::UNAUTHORIZED || status == StatusCode::FORBIDDEN => {
            OcrError::Auth
        }
        _ if status == StatusCode::TOO_MANY_REQUESTS => OcrError::RateLimited(detail),
        _ if status == StatusCode::BAD_REQUEST
            || status == StatusCode::PAYLOAD_TOO_LARGE
            || status == StatusCode::UNPROCESSABLE_ENTITY =>
        {
            OcrError::InvalidInput(detail)
        }
        _ if status.is_server_error() => OcrError::Server(detail),
        _ => OcrError::Server(detail),
    }
}

fn redact(mut value: String, token: &str) -> String {
    if !token.is_empty() {
        value = value.replace(token, "[redacted]");
    }
    value
}

#[cfg(test)]
mod tests {
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Mutex,
    };

    use wiremock::{
        matchers::{method, path},
        Mock, MockServer, Request, Respond, ResponseTemplate,
    };

    use super::*;

    const RESULT_JSONL: &str = r#"{"result":{"dataInfo":{"width":100,"height":50},"ocrResults":[{"prunedResult":{"rec_texts":["hello"],"rec_boxes":[[1,2,20,10]]}}]}}"#;

    struct PollSequence {
        calls: AtomicUsize,
        result_url: String,
    }

    impl Respond for PollSequence {
        fn respond(&self, _request: &Request) -> ResponseTemplate {
            match self.calls.fetch_add(1, Ordering::SeqCst) {
                0 => ResponseTemplate::new(200).set_body_json(json!({
                    "code": 0,
                    "data": { "state": "pending" }
                })),
                1 => ResponseTemplate::new(200).set_body_json(json!({
                    "code": 0,
                    "data": {
                        "state": "running",
                        "extractProgress": { "totalPages": "1", "extractedPages": "1" }
                    }
                })),
                _ => ResponseTemplate::new(200).set_body_json(json!({
                    "code": 0,
                    "data": {
                        "state": "done",
                        "resultUrl": { "jsonUrl": self.result_url }
                    }
                })),
            }
        }
    }

    #[test]
    fn splits_large_pdfs_into_deterministic_ranges() {
        assert_eq!(page_ranges(201), vec!["1-100", "101-200", "201"]);
    }

    #[test]
    fn maps_documented_failures() {
        assert!(matches!(
            map_failure(StatusCode::FORBIDDEN, Some(12001), "quota".into()),
            OcrError::Quota
        ));
        assert!(matches!(
            map_failure(StatusCode::OK, Some(10010), "busy".into()),
            OcrError::RateLimited(_)
        ));
        assert!(matches!(
            map_failure(StatusCode::BAD_REQUEST, Some(10003), "large".into()),
            OcrError::InvalidInput(_)
        ));
        assert!(matches!(
            map_failure(StatusCode::UNAUTHORIZED, None, String::new()),
            OcrError::Auth
        ));
        assert!(matches!(
            map_failure(StatusCode::INTERNAL_SERVER_ERROR, None, "upstream".into()),
            OcrError::Server(_)
        ));
    }

    #[tokio::test]
    async fn submits_polls_downloads_and_reuses_checkpoint() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path(JOBS_PATH))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "code": 0,
                "data": { "jobId": "job-1" }
            })))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path(format!("{JOBS_PATH}/job-1")))
            .respond_with(PollSequence {
                calls: AtomicUsize::new(0),
                result_url: format!("{}/result.jsonl", server.uri()),
            })
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/result.jsonl"))
            .respond_with(ResponseTemplate::new(200).set_body_string(RESULT_JSONL))
            .mount(&server)
            .await;

        let directory = tempfile::tempdir().unwrap();
        let input_path = directory.path().join("sample.png");
        std::fs::write(&input_path, b"image").unwrap();
        let service = PaddleOcr::with_endpoint(
            ServiceId::PpOcrV6,
            Arc::new(|| Ok("test-token".into())),
            Arc::new(|| Ok(ProxyConfig::Direct)),
            server.uri(),
            Duration::from_millis(1),
        );
        let checkpoint = ParseCheckpoint::empty();
        let observed = Arc::new(Mutex::new(Vec::new()));
        let progress = Arc::clone(&observed);
        let result = service
            .parse_resumable(
                &InputDoc { path: input_path },
                &ParseOptions::default(),
                Box::new(move |page, total| progress.lock().unwrap().push((page, total))),
                checkpoint.clone(),
            )
            .await
            .unwrap();

        assert_eq!(result.markdown, "hello");
        assert_eq!(checkpoint.job_ids().unwrap(), ["job-1"]);
        assert!(observed.lock().unwrap().contains(&(1, 1)));
        let requests = server.received_requests().await.unwrap();
        let post = requests
            .iter()
            .find(|request| request.method.as_str() == "POST")
            .unwrap();
        let multipart = String::from_utf8_lossy(&post.body);
        assert!(multipart.contains("PP-OCRv6"));
        assert!(multipart.contains("optionalPayload"));

        let resumed = ParseCheckpoint::new(vec!["job-1".into()], Arc::new(|_| Ok(())));
        service
            .parse_resumable(
                &InputDoc {
                    path: directory.path().join("unused.png"),
                },
                &ParseOptions::default(),
                Box::new(|_, _| {}),
                resumed,
            )
            .await
            .unwrap();
        let requests = server.received_requests().await.unwrap();
        assert_eq!(
            requests
                .iter()
                .filter(|request| request.method.as_str() == "POST")
                .count(),
            1
        );
    }

    #[tokio::test]
    async fn replaces_an_expired_checkpoint() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path(format!("{JOBS_PATH}/expired")))
            .respond_with(ResponseTemplate::new(400).set_body_json(json!({
                "code": 11002,
                "msg": "expired"
            })))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path(JOBS_PATH))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "code": 0,
                "data": { "jobId": "replacement" }
            })))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path(format!("{JOBS_PATH}/replacement")))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "code": 0,
                "data": {
                    "state": "done",
                    "resultUrl": { "jsonUrl": format!("{}/result.jsonl", server.uri()) }
                }
            })))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/result.jsonl"))
            .respond_with(ResponseTemplate::new(200).set_body_string(RESULT_JSONL))
            .mount(&server)
            .await;

        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("sample.png");
        std::fs::write(&path, b"image").unwrap();
        let service = PaddleOcr::with_endpoint(
            ServiceId::PpOcrV6,
            Arc::new(|| Ok("test-token".into())),
            Arc::new(|| Ok(ProxyConfig::Direct)),
            server.uri(),
            Duration::from_millis(1),
        );
        let checkpoint = ParseCheckpoint::new(vec!["expired".into()], Arc::new(|_| Ok(())));
        service
            .parse_resumable(
                &InputDoc { path },
                &ParseOptions::default(),
                Box::new(|_, _| {}),
                checkpoint.clone(),
            )
            .await
            .unwrap();

        assert_eq!(checkpoint.job_ids().unwrap(), ["replacement"]);
    }

    #[tokio::test]
    async fn custom_proxy_changes_the_request_path() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/health"))
            .respond_with(ResponseTemplate::new(200))
            .mount(&server)
            .await;
        let url = format!("{}/health", server.uri());

        assert!(build_client(ProxyConfig::Direct)
            .unwrap()
            .get(&url)
            .send()
            .await
            .is_ok());
        assert!(
            build_client(ProxyConfig::Custom("http://127.0.0.1:1".into()))
                .unwrap()
                .get(&url)
                .send()
                .await
                .is_err()
        );
    }
}
