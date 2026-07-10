use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ServiceId {
    Vl16,
    PpOcrV6,
    StructureV3,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BlockKind {
    Text,
    Table,
    Formula,
    Seal,
    Chart,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Block {
    pub id: String,
    pub kind: BlockKind,
    /// 上游原始坐标系原样保留；归一化在渲染层做
    pub bbox: Option<[f32; 4]>,
    /// Text=纯文本 Table=CSV Formula=LaTeX
    pub content: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Page {
    pub width: f32,
    pub height: f32,
    pub blocks: Vec<Block>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RecognitionResult {
    pub markdown: String,
    pub page_count: u32,
    pub pages: Vec<Page>,
}

#[derive(Debug, Clone, thiserror::Error, Serialize, Deserialize)]
#[serde(tag = "kind", content = "detail", rename_all = "snake_case")]
pub enum OcrError {
    #[error("authentication failed")]
    Auth,
    #[error("daily quota exhausted")]
    Quota,
    #[error("rate limited: {0}")]
    RateLimited(String),
    #[error("invalid input: {0}")]
    InvalidInput(String),
    #[error("network error: {0}")]
    Network(String),
    #[error("server error: {0}")]
    Server(String),
    #[error("internal error: {0}")]
    Internal(String),
    #[error("response parse failed: {0}")]
    Parse(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn result_serde_roundtrip() {
        let r = RecognitionResult {
            markdown: "# t".into(),
            page_count: 1,
            pages: vec![Page {
                width: 595.0,
                height: 842.0,
                blocks: vec![Block {
                    id: "b1".into(),
                    kind: BlockKind::Table,
                    bbox: Some([10.0, 20.0, 300.0, 200.0]),
                    content: "a,b\n1,2".into(),
                }],
            }],
        };
        let json = serde_json::to_string(&r).unwrap();
        assert_eq!(serde_json::from_str::<RecognitionResult>(&json).unwrap(), r);
        assert!(json.contains("\"table\"")); // kind 序列化为 snake_case
    }
}
