use crate::model::{BlockKind, RecognitionResult};

pub fn export(
    result: &RecognitionResult,
    format: &str,
    block_id: Option<&str>,
) -> Result<Vec<u8>, String> {
    match format {
        "md" => Ok(result.markdown.as_bytes().to_vec()),
        "json" => serde_json::to_vec_pretty(result).map_err(|error| error.to_string()),
        "txt" => Ok(result
            .pages
            .iter()
            .flat_map(|page| &page.blocks)
            .filter(|block| block.kind == BlockKind::Text)
            .map(|block| block.content.as_str())
            .collect::<Vec<_>>()
            .join("\n\n")
            .into_bytes()),
        "csv" => {
            let id = block_id.ok_or_else(|| "CSV export requires block_id".to_string())?;
            let block = result
                .pages
                .iter()
                .flat_map(|page| &page.blocks)
                .find(|block| block.id == id && block.kind == BlockKind::Table)
                .ok_or_else(|| "table block not found".to_string())?;
            let mut output = vec![0xEF, 0xBB, 0xBF];
            output.extend_from_slice(block.content.as_bytes());
            Ok(output)
        }
        _ => Err(format!("unsupported export format: {format}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Block, BlockKind, Page, RecognitionResult};

    fn sample() -> RecognitionResult {
        RecognitionResult {
            markdown: "# 你好".into(),
            page_count: 1,
            pages: vec![Page {
                width: 100.0,
                height: 100.0,
                blocks: vec![
                    Block {
                        id: "t1".into(),
                        kind: BlockKind::Text,
                        bbox: None,
                        content: "你好".into(),
                    },
                    Block {
                        id: "tb1".into(),
                        kind: BlockKind::Table,
                        bbox: None,
                        content: "a,b\n1,2".into(),
                    },
                ],
            }],
        }
    }

    #[test]
    fn csv_export_has_bom_and_content() {
        let out = export(&sample(), "csv", Some("tb1")).unwrap();
        assert_eq!(&out[..3], &[0xEF, 0xBB, 0xBF]);
        assert_eq!(&out[3..], "a,b\n1,2".as_bytes());
    }

    #[test]
    fn txt_export_joins_text_blocks() {
        assert_eq!(export(&sample(), "txt", None).unwrap(), "你好".as_bytes());
    }

    #[test]
    fn csv_on_non_table_errors() {
        assert!(export(&sample(), "csv", Some("t1")).is_err());
    }
}
