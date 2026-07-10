pub mod mock;
pub mod normalize;

use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
};

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
pub type CheckpointFn = Arc<dyn Fn(&[String]) -> Result<(), OcrError> + Send + Sync>;

#[derive(Clone)]
pub struct ParseCheckpoint {
    job_ids: Arc<Mutex<Vec<String>>>,
    persist: CheckpointFn,
}

impl ParseCheckpoint {
    pub fn empty() -> Self {
        Self::new(Vec::new(), Arc::new(|_| Ok(())))
    }

    pub(crate) fn new(job_ids: Vec<String>, persist: CheckpointFn) -> Self {
        Self {
            job_ids: Arc::new(Mutex::new(job_ids)),
            persist,
        }
    }

    pub fn job_ids(&self) -> Result<Vec<String>, OcrError> {
        self.job_ids
            .lock()
            .map(|ids| ids.clone())
            .map_err(|_| OcrError::Parse("job checkpoint lock poisoned".into()))
    }

    pub fn save(&self, job_ids: Vec<String>) -> Result<(), OcrError> {
        (self.persist)(&job_ids)?;
        *self
            .job_ids
            .lock()
            .map_err(|_| OcrError::Parse("job checkpoint lock poisoned".into()))? = job_ids;
        Ok(())
    }
}

#[async_trait::async_trait]
pub trait OcrService: Send + Sync {
    fn id(&self) -> ServiceId;
    async fn parse(
        &self,
        input: &InputDoc,
        opts: &ParseOptions,
        progress: ProgressFn,
    ) -> Result<RecognitionResult, OcrError>;

    async fn parse_resumable(
        &self,
        input: &InputDoc,
        opts: &ParseOptions,
        progress: ProgressFn,
        _checkpoint: ParseCheckpoint,
    ) -> Result<RecognitionResult, OcrError> {
        self.parse(input, opts, progress).await
    }
}
