pub mod mock;
pub mod normalize;

use std::path::PathBuf;

use crate::model::{OcrError, RecognitionResult, ServiceId};
use serde::{Deserialize, Serialize};

pub struct InputDoc {
    pub path: PathBuf,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ParseOptions {
    pub lang: Option<String>,
}

/// 页级进度回调 (extracted_pages, total_pages)——真实 API 为异步任务轮询，
/// 进度产生在 parse 内部（见 docs/api/paddleocr-api.md），故回调进 trait
pub type ProgressFn = Box<dyn Fn(u32, u32) + Send + Sync>;

#[async_trait::async_trait]
pub trait OcrService: Send + Sync {
    fn id(&self) -> ServiceId;
    async fn parse(
        &self,
        input: &InputDoc,
        opts: &ParseOptions,
        progress: ProgressFn,
    ) -> Result<RecognitionResult, OcrError>;
}
