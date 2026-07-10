use scraper::{Html, Selector};
use serde::Deserialize;

use crate::model::{Block, BlockKind, OcrError, Page, RecognitionResult, ServiceId};

#[derive(Deserialize)]
struct Envelope {
    result: RawResult,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawResult {
    data_info: DataInfo,
    #[serde(default)]
    ocr_results: Vec<OcrPage>,
    #[serde(default)]
    layout_parsing_results: Vec<LayoutPage>,
}

#[derive(Default, Deserialize)]
struct DataInfo {
    width: Option<f32>,
    height: Option<f32>,
    #[serde(default)]
    pages: Vec<PageSize>,
}

#[derive(Deserialize)]
struct PageSize {
    width: f32,
    height: f32,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct OcrPage {
    pruned_result: OcrPruned,
}

#[derive(Deserialize)]
struct OcrPruned {
    rec_texts: Vec<String>,
    rec_boxes: Vec<[f32; 4]>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct LayoutPage {
    markdown: Markdown,
    pruned_result: LayoutPruned,
}

#[derive(Deserialize)]
struct Markdown {
    text: String,
}

#[derive(Deserialize)]
struct LayoutPruned {
    width: f32,
    height: f32,
    parsing_res_list: Vec<LayoutBlock>,
}

#[derive(Deserialize)]
struct LayoutBlock {
    block_id: i64,
    block_label: String,
    block_content: String,
    block_bbox: Option<[f32; 4]>,
}

pub fn normalize_jsonl(service: ServiceId, body: &str) -> Result<RecognitionResult, OcrError> {
    let mut pages = Vec::new();
    let mut markdown = Vec::new();

    for line in body.lines().filter(|line| !line.trim().is_empty()) {
        let envelope: Envelope = serde_json::from_str(line)
            .map_err(|error| OcrError::Parse(format!("invalid JSONL: {error}")))?;
        match service {
            ServiceId::PpOcrV6 => normalize_ocr(envelope.result, &mut pages, &mut markdown)?,
            ServiceId::Vl16 | ServiceId::StructureV3 => {
                normalize_layout(envelope.result, &mut pages, &mut markdown)
            }
        }
    }

    if pages.is_empty() {
        return Err(OcrError::Parse("response contains no pages".into()));
    }

    Ok(RecognitionResult {
        markdown: markdown.join("\n\n"),
        page_count: pages.len() as u32,
        pages,
    })
}

fn normalize_ocr(
    result: RawResult,
    pages: &mut Vec<Page>,
    markdown: &mut Vec<String>,
) -> Result<(), OcrError> {
    for (local_index, raw_page) in result.ocr_results.into_iter().enumerate() {
        if raw_page.pruned_result.rec_texts.len() != raw_page.pruned_result.rec_boxes.len() {
            return Err(OcrError::Parse(
                "PP-OCRv6 text and box counts differ".into(),
            ));
        }
        let size = result.data_info.pages.get(local_index);
        let width = size
            .map(|page| page.width)
            .or(result.data_info.width)
            .ok_or_else(|| OcrError::Parse("PP-OCRv6 page width is missing".into()))?;
        let height = size
            .map(|page| page.height)
            .or(result.data_info.height)
            .ok_or_else(|| OcrError::Parse("PP-OCRv6 page height is missing".into()))?;
        let page_index = pages.len();
        let mut text = Vec::new();
        let blocks = raw_page
            .pruned_result
            .rec_texts
            .into_iter()
            .zip(raw_page.pruned_result.rec_boxes)
            .enumerate()
            .map(|(index, (content, bbox))| {
                text.push(content.clone());
                Block {
                    id: format!("p{page_index}-text-{index}"),
                    kind: BlockKind::Text,
                    bbox: Some(bbox),
                    content,
                }
            })
            .collect();
        markdown.push(text.join("\n"));
        pages.push(Page {
            width,
            height,
            blocks,
        });
    }
    Ok(())
}

fn normalize_layout(result: RawResult, pages: &mut Vec<Page>, markdown: &mut Vec<String>) {
    for raw_page in result.layout_parsing_results {
        let page_index = pages.len();
        markdown.push(raw_page.markdown.text);
        let blocks = raw_page
            .pruned_result
            .parsing_res_list
            .into_iter()
            .map(|raw| {
                let kind = block_kind(&raw.block_label);
                let content = if kind == BlockKind::Table {
                    html_table_to_csv(&raw.block_content)
                } else {
                    raw.block_content
                };
                Block {
                    id: format!("p{page_index}-b{}", raw.block_id),
                    kind,
                    bbox: raw.block_bbox,
                    content,
                }
            })
            .collect();
        pages.push(Page {
            width: raw_page.pruned_result.width,
            height: raw_page.pruned_result.height,
            blocks,
        });
    }
}

fn block_kind(label: &str) -> BlockKind {
    let label = label.to_ascii_lowercase();
    if label.contains("table") {
        BlockKind::Table
    } else if label.contains("formula") {
        BlockKind::Formula
    } else if label.contains("seal") {
        BlockKind::Seal
    } else if label.contains("chart") {
        BlockKind::Chart
    } else {
        BlockKind::Text
    }
}

fn html_table_to_csv(input: &str) -> String {
    let fragment = Html::parse_fragment(input);
    let rows = Selector::parse("tr").expect("constant row selector is valid");
    let cells = Selector::parse("th, td").expect("constant cell selector is valid");
    let csv = fragment
        .select(&rows)
        .map(|row| {
            row.select(&cells)
                .map(|cell| csv_cell(&cell.text().collect::<String>()))
                .collect::<Vec<_>>()
                .join(",")
        })
        .collect::<Vec<_>>()
        .join("\n");
    if csv.is_empty() {
        input.to_string()
    } else {
        csv
    }
}

fn csv_cell(value: &str) -> String {
    let value = value.trim();
    if value.contains([',', '"', '\r', '\n']) {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::normalize_jsonl;
    use crate::model::{BlockKind, ServiceId};

    #[test]
    fn ppocr_fixture_maps_text_and_pixel_boxes() {
        let raw = include_str!("../../tests/fixtures/ppocrv6_image.json");
        let result = normalize_jsonl(ServiceId::PpOcrV6, raw).unwrap();

        assert_eq!(result.page_count, 1);
        assert_eq!(
            (result.pages[0].width, result.pages[0].height),
            (1200.0, 800.0)
        );
        assert_eq!(
            result.pages[0].blocks[0].bbox,
            Some([96.0, 96.0, 419.0, 144.0])
        );
        assert!(!result.markdown.is_empty());
    }

    #[test]
    fn layout_fixtures_map_pages_tables_and_stable_ids() {
        let vl = normalize_jsonl(
            ServiceId::Vl16,
            include_str!("../../tests/fixtures/vl16_pdf.json"),
        )
        .unwrap();
        assert_eq!(vl.page_count, 2);
        assert!(vl
            .pages
            .iter()
            .flat_map(|page| &page.blocks)
            .any(|block| block.kind == BlockKind::Table && block.content.contains(',')));
        assert_eq!(vl.pages[0].blocks[0].id, "p0-b0");

        let structure = normalize_jsonl(
            ServiceId::StructureV3,
            include_str!("../../tests/fixtures/structurev3_image.json"),
        )
        .unwrap();
        assert!(structure.pages[0]
            .blocks
            .iter()
            .all(|block| block.bbox.is_some()));
    }

    #[test]
    fn malformed_jsonl_is_a_parse_error() {
        assert!(normalize_jsonl(ServiceId::Vl16, "not json").is_err());
    }
}
