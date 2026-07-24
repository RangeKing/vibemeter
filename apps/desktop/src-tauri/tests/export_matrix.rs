use aftervibe_lib::database::Database;
use aftervibe_lib::export;
use aftervibe_lib::models::{ExportRequest, ShareRenderRequest};
use std::path::PathBuf;

fn without_embedded_image_data(svg: &str) -> String {
    let mut sanitized = String::with_capacity(svg.len());
    let mut remaining = svg;
    while let Some(start) = remaining.find("data:image/") {
        sanitized.push_str(&remaining[..start]);
        let data_uri = &remaining[start..];
        let Some(end) = data_uri.find('"') else {
            break;
        };
        sanitized.push_str("data:image/omitted");
        remaining = &data_uri[end..];
    }
    sanitized.push_str(remaining);
    sanitized
}

#[test]
#[ignore = "requires AFTERVIBE_TEST_DB with a sanitized or local aftervibe database snapshot"]
fn validates_the_288_release_export_combinations() {
    let database_path = std::env::var("AFTERVIBE_TEST_DB")
        .map(PathBuf::from)
        .expect("AFTERVIBE_TEST_DB");
    let output = std::env::var("AFTERVIBE_EXPORT_MATRIX_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| std::env::temp_dir().join("aftervibe-export-matrix"));
    std::fs::create_dir_all(&output).expect("matrix output directory");
    let database = Database::open(database_path).expect("database snapshot");

    let template_filter = std::env::var("AFTERVIBE_EXPORT_TEMPLATE").ok();
    let templates = [
        "usage-overview",
        "developer-wrapped",
        "agent-comparison",
        "session-recap",
        "daily-review",
        "session-breakdown",
        "weekly-recap",
        "ship-card",
        "vcti-card",
    ]
    .into_iter()
    .filter(|template| {
        template_filter
            .as_deref()
            .is_none_or(|value| value == *template)
    })
    .collect::<Vec<_>>();
    let locales = ["en-US", "zh-CN"];
    let aspects = ["1:1", "2:3", "3:2", "3:4", "4:3", "4:5", "16:9", "9:16"];
    let formats = ["svg", "png"];
    let mut count = 0;

    let expected_count = templates.len() * locales.len() * aspects.len() * formats.len();
    for template in templates {
        for locale in locales {
            for aspect in aspects {
                let render = ShareRenderRequest {
                    template_id: template.into(),
                    locale: locale.into(),
                    aspect_ratio: aspect.into(),
                    theme: if locale == "zh-CN" { "dark" } else { "light" }.into(),
                    range: if template == "vcti-card" {
                        "90d"
                    } else {
                        "30d"
                    }
                    .into(),
                    session_id: None,
                    compare_ids: Vec::new(),
                    title: String::new(),
                    summary: String::new(),
                    project_name: String::new(),
                    metrics: Vec::new(),
                    show_brand: true,
                    show_model: true,
                    show_cost: true,
                    show_project: false,
                    show_behavior_evidence: template == "vcti-card",
                    privacy_reviewed: true,
                };
                let first = export::preview(&database, render.clone()).expect("first preview");
                let second = export::preview(&database, render.clone()).expect("second preview");
                assert_eq!(first.svg, second.svg, "preview must be deterministic");
                assert_eq!(first.model_hash, second.model_hash);
                assert!(first.can_export, "default sanitized model should export");
                assert!(first.svg.starts_with("<svg"));
                let inspectable_svg = without_embedded_image_data(&first.svg);
                assert!(!inspectable_svg.contains("undefined"));
                if inspectable_svg.contains("NaN") {
                    let safe_aspect = aspect.replace(':', "x");
                    std::fs::write(
                        output.join(format!("{template}_{locale}_{safe_aspect}.invalid.svg")),
                        &first.svg,
                    )
                    .expect("invalid preview diagnostic");
                }
                assert!(
                    !inspectable_svg.contains("NaN"),
                    "{template} {locale} {aspect} preview contains NaN"
                );
                resvg::usvg::Tree::from_data(
                    first.svg.as_bytes(),
                    &resvg::usvg::Options::default(),
                )
                .expect("valid vector document");

                for format in formats {
                    let safe_aspect = aspect.replace(':', "x");
                    let path = output.join(format!("{template}_{locale}_{safe_aspect}.{format}"));
                    let result = export::export(
                        &database,
                        ExportRequest {
                            render: render.clone(),
                            format: format.into(),
                            path: path.to_string_lossy().to_string(),
                        },
                    )
                    .expect("matrix export");
                    assert_eq!(result.model_hash, first.model_hash);
                    assert!(result.bytes_written > 512);
                    let bytes = std::fs::read(&path).expect("export file");
                    if format == "png" {
                        assert_eq!(&bytes[..8], b"\x89PNG\r\n\x1a\n");
                    } else {
                        assert!(bytes.starts_with(b"<svg"));
                    }
                    count += 1;
                }
            }
        }
    }
    assert_eq!(count, expected_count);
    println!(
        "validated {count} real-data export files in {}",
        output.display()
    );
}
