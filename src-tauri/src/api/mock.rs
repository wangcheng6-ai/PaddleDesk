#[cfg(test)]
use std::sync::atomic::AtomicU32 as CallCounter;
use std::sync::{
    atomic::{AtomicU32 as FailureCounter, Ordering},
    Arc,
};

use super::*;
use crate::model::*;

#[derive(Clone, Default)]
pub struct MockOcr {
    remaining_failures: Arc<FailureCounter>,
    err: Option<OcrError>,
    #[cfg(test)]
    calls: Arc<CallCounter>,
}

impl MockOcr {
    pub fn new() -> Self {
        Self {
            remaining_failures: Arc::new(FailureCounter::new(0)),
            err: None,
            #[cfg(test)]
            calls: Arc::default(),
        }
    }

    pub fn failing(times: u32, err: OcrError) -> Self {
        Self {
            remaining_failures: Arc::new(FailureCounter::new(times)),
            err: Some(err),
            #[cfg(test)]
            calls: Arc::default(),
        }
    }

    #[cfg(test)]
    pub fn call_count(&self) -> u32 {
        self.calls.load(Ordering::SeqCst)
    }

    fn canned() -> RecognitionResult {
        RecognitionResult {
            markdown: "# Mock 文档\n\n这是模拟识别结果。".into(),
            page_count: 1,
            pages: vec![Page {
                width: 595.0,
                height: 842.0,
                blocks: vec![Block {
                    id: "b1".into(),
                    kind: BlockKind::Text,
                    bbox: Some([50.0, 50.0, 545.0, 90.0]),
                    content: "这是模拟识别结果。".into(),
                }],
            }],
        }
    }
}

#[async_trait::async_trait]
impl OcrService for MockOcr {
    fn id(&self) -> ServiceId {
        ServiceId::Vl16
    }

    async fn parse(
        &self,
        _i: &InputDoc,
        _o: &ParseOptions,
        progress: ProgressFn,
    ) -> Result<RecognitionResult, OcrError> {
        #[cfg(test)]
        self.calls.fetch_add(1, Ordering::SeqCst);
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        if self
            .remaining_failures
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |n| n.checked_sub(1))
            .is_ok()
        {
            return Err(self.err.clone().unwrap());
        }
        progress(1, 1);
        Ok(Self::canned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn noop() -> ProgressFn {
        Box::new(|_, _| {})
    }

    #[tokio::test]
    async fn mock_returns_one_page_and_reports_progress() {
        let svc = MockOcr::new();
        let hits = std::sync::Arc::new(std::sync::atomic::AtomicU32::new(0));
        let h = hits.clone();
        let r = svc
            .parse(
                &InputDoc {
                    path: "x.png".into(),
                },
                &ParseOptions::default(),
                Box::new(move |_, _| {
                    h.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                }),
            )
            .await
            .unwrap();
        assert_eq!(r.page_count, 1);
        assert!(!r.pages[0].blocks.is_empty());
        assert!(hits.load(std::sync::atomic::Ordering::SeqCst) >= 1);
    }

    #[tokio::test]
    async fn mock_fails_then_succeeds() {
        let svc = MockOcr::failing(2, OcrError::Network("timeout".into()));
        assert!(svc
            .parse(
                &InputDoc {
                    path: "x.png".into(),
                },
                &Default::default(),
                noop(),
            )
            .await
            .is_err());
        assert!(svc
            .parse(
                &InputDoc {
                    path: "x.png".into(),
                },
                &Default::default(),
                noop(),
            )
            .await
            .is_err());
        assert!(svc
            .parse(
                &InputDoc {
                    path: "x.png".into(),
                },
                &Default::default(),
                noop(),
            )
            .await
            .is_ok());
    }
}
