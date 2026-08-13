use std::path::PathBuf;
use vibemeter_lib::database::Database;
use vibemeter_lib::export;
use vibemeter_lib::models::{ExportRequest, ShareRenderRequest};

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
#[ignore = "requires VIBEMETER_TEST_DB with a sanitized or local VibeMeter database snapshot"]
fn validates_the_release_export_matrix() {
    let database_path = std::env::var("VIBEMETER_TEST_DB")
        .or_else(|_| std::env::var("AFTERVIBE_TEST_DB"))
        .or_else(|_| std::env::var("TOKEN_GRAPH_TEST_DB"))
        .map(PathBuf::from)
        .expect("VIBEMETER_TEST_DB");
    let output = std::env::var("VIBEMETER_EXPORT_MATRIX_DIR")
        .or_else(|_| std::env::var("AFTERVIBE_EXPORT_MATRIX_DIR"))
        .or_else(|_| std::env::var("TOKEN_GRAPH_EXPORT_MATRIX_DIR"))
        .map(PathBuf::from)
        .unwrap_or_else(|_| std::env::temp_dir().join("vibemeter-export-matrix"));
    std::fs::create_dir_all(&output).expect("matrix output directory");
    let database = Database::open(database_path).expect("database snapshot");

    let template_filter = std::env::var("VIBEMETER_EXPORT_TEMPLATE")
        .or_else(|_| std::env::var("AFTERVIBE_EXPORT_TEMPLATE"))
        .ok();
    let templates = [
        "usage-overview",
        "developer-wrapped",
        "agent-comparison",
        "session-recap",
        "vcti-card",
        "catchphrases",
    ]
    .into_iter()
    .filter(|template| {
        template_filter
            .as_deref()
            .is_none_or(|value| value == *template)
    })
    .collect::<Vec<_>>();
    let locales = ["en-US", "zh-CN"];
    let themes = ["light", "dark"];
    let aspects = ["1:1", "2:3", "3:2", "3:4", "4:3", "4:5", "16:9", "9:16"];
    let formats = ["svg", "png"];
    let mut count = 0;

    let expected_count =
        templates.len() * locales.len() * themes.len() * aspects.len() * formats.len();
    for template in templates {
        for locale in locales {
            for theme in themes {
                for aspect in aspects {
                    let render = ShareRenderRequest {
                        template_id: template.into(),
                        locale: locale.into(),
                        aspect_ratio: aspect.into(),
                        theme: theme.into(),
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
                    let second =
                        export::preview(&database, render.clone()).expect("second preview");
                    assert_eq!(first.svg, second.svg, "preview must be deterministic");
                    assert_eq!(first.model_hash, second.model_hash);
                    assert!(first.can_export, "default sanitized model should export");
                    assert!(first.svg.starts_with("<svg"));
                    if template == "vcti-card" {
                        assert!(first.svg.contains("data:image/webp;base64,"));
                        assert!(first.svg.contains("data-vcti-visual-version=\"2.0.0\""));
                        assert!(!first.svg.contains("Fingerprint"));
                        let evidence_labels = if locale == "zh-CN" {
                            ["工作节奏", "协作方式", "工具与 Skill", "过程记录"]
                        } else {
                            [
                                "WORK RHYTHM",
                                "COLLABORATION",
                                "TOOLS &amp; SKILL",
                                "PROCESS RECORD",
                            ]
                        };
                        for label in evidence_labels {
                            assert!(
                                first.svg.contains(label),
                                "VCTI {locale} {aspect} preview is missing {label}"
                            );
                        }
                        for forbidden in ["/Users/", "SKILL.md", "apply_patch", "exec_command"] {
                            assert!(!first.svg.contains(forbidden), "private token in VCTI SVG");
                        }
                    }
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
                        let path = output.join(format!(
                            "{template}_{locale}_{theme}_{safe_aspect}.{format}"
                        ));
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
                            let expected = export_dimensions(aspect);
                            assert_eq!(
                                u32::from_be_bytes(bytes[16..20].try_into().unwrap()),
                                expected.0
                            );
                            assert_eq!(
                                u32::from_be_bytes(bytes[20..24].try_into().unwrap()),
                                expected.1
                            );
                        } else {
                            assert!(bytes.starts_with(b"<svg"));
                        }
                        count += 1;
                    }
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

fn export_dimensions(aspect: &str) -> (u32, u32) {
    match aspect {
        "1:1" => (2400, 2400),
        "2:3" => (1920, 2880),
        "3:2" => (2880, 1920),
        "3:4" => (2160, 2880),
        "4:3" => (2400, 1800),
        "4:5" => (2160, 2700),
        "16:9" => (2560, 1440),
        "9:16" => (1440, 2560),
        _ => panic!("unexpected aspect"),
    }
}
