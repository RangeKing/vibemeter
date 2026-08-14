use crate::database::Database;
use crate::errors::{AppError, AppResult};
use crate::export_localization as loc;
use crate::models::{
    ComparisonItem, ExportRequest, ExportResult, IndexStatus, OverviewResponse,
    PhraseCloudResponse, SessionDetail, SharePreview, ShareRenderRequest, VctiEvidenceItem,
    VctiOptionalMetric, VctiProfile,
};
use crate::privacy;
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64_STANDARD};
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::Path;
use uuid::Uuid;

const TEMPLATE_VERSION: &str = "17.0.0";
const SAVITZKY_GOLAY_7: [f64; 7] = [
    -2.0 / 21.0,
    3.0 / 21.0,
    6.0 / 21.0,
    7.0 / 21.0,
    6.0 / 21.0,
    3.0 / 21.0,
    -2.0 / 21.0,
];
const SAVITZKY_GOLAY_5: [f64; 5] = [
    -3.0 / 35.0,
    12.0 / 35.0,
    17.0 / 35.0,
    12.0 / 35.0,
    -3.0 / 35.0,
];

#[derive(Clone, Copy)]
struct Palette {
    background: &'static str,
    surface: &'static str,
    surface_strong: &'static str,
    text: &'static str,
    muted: &'static str,
    hairline: &'static str,
    accent: &'static str,
    accent_soft: &'static str,
    accent_alt: &'static str,
    claude: &'static str,
    codex: &'static str,
    kimi: &'static str,
    zcode: &'static str,
    positive: &'static str,
    tile: &'static str,
    dark: bool,
}

impl Palette {
    fn for_theme(theme: &str) -> Self {
        if theme == "dark" {
            Self {
                background: "#070708",
                surface: "#070708",
                surface_strong: "#F5F5F7",
                text: "#F5F5F7",
                muted: "#A1A1A8",
                hairline: "#343438",
                accent: "#78A9FF",
                accent_soft: "#17315D",
                accent_alt: "#78A9FF",
                claude: "#78A9FF",
                codex: "#78A9FF",
                kimi: "#78A9FF",
                zcode: "#F2B66D",
                positive: "#78A9FF",
                tile: "#1C1C1F",
                dark: true,
            }
        } else {
            Self {
                background: "#FFFFFF",
                surface: "#FFFFFF",
                surface_strong: "#FFFFFF",
                text: "#111113",
                muted: "#595A61",
                hairline: "#D7D7DB",
                accent: "#075FDB",
                accent_soft: "#DCEAFF",
                accent_alt: "#075FDB",
                claude: "#075FDB",
                codex: "#075FDB",
                kimi: "#075FDB",
                zcode: "#B86B2D",
                positive: "#075FDB",
                tile: "#EEEEF0",
                dark: false,
            }
        }
    }
}

struct ShareData {
    overview: OverviewResponse,
    comparison: Vec<ComparisonItem>,
    session: Option<SessionDetail>,
    vcti: Option<VctiProfile>,
    phrases: Option<PhraseCloudResponse>,
}

pub fn preview(database: &Database, request: ShareRenderRequest) -> AppResult<SharePreview> {
    validate_request(&request)?;
    let data = load_data(database, &request)?;
    let mut findings = privacy::inspect_share(&request);
    if data.session.is_some() && !request.privacy_reviewed {
        findings.retain(|finding| finding.id != "safe");
        findings.push(crate::models::ShareGuardFinding {
            id: "source-title-review".into(),
            level: "review".into(),
            message_key: "share.guard.sessionTitleReview".into(),
        });
    }
    let can_export = privacy::export_allowed(&findings, request.privacy_reviewed);
    let (width, height) = dimensions(&request.aspect_ratio)?;
    let svg = render(&request, &data, width, height);
    let model_hash = privacy::stable_hash(&format!("{TEMPLATE_VERSION}:{svg}"));
    Ok(SharePreview {
        svg,
        width,
        height,
        findings,
        can_export,
        model_hash,
    })
}

pub fn export(database: &Database, request: ExportRequest) -> AppResult<ExportResult> {
    let preview = preview(database, request.render.clone())?;
    if !preview.can_export {
        return Err(AppError::PrivacyBlocked);
    }
    let format = request.format.to_ascii_lowercase();
    let output = Path::new(&request.path);
    if request.path.trim().is_empty() || output.file_name().is_none() {
        return Err(AppError::InvalidRequest("export path is invalid".into()));
    }
    match format.as_str() {
        "svg" => std::fs::write(output, preview.svg.as_bytes())?,
        "png" => render_png(&preview.svg, output)?,
        _ => return Err(AppError::UnsupportedExport),
    }
    let bytes_written = std::fs::metadata(output)?.len();
    database.record_export(
        &Uuid::new_v4().to_string(),
        &request.render.template_id,
        loc::normalize_locale(&request.render.locale),
        &request.render.aspect_ratio,
        &format,
        &preview.model_hash,
    )?;
    Ok(ExportResult {
        path: request.path,
        format,
        width: preview.width,
        height: preview.height,
        bytes_written,
        model_hash: preview.model_hash,
    })
}

pub fn png_bytes(database: &Database, request: ShareRenderRequest) -> AppResult<Vec<u8>> {
    let preview = preview(database, request)?;
    if !preview.can_export {
        return Err(AppError::PrivacyBlocked);
    }
    render_png_bytes(&preview.svg)
}

fn load_data(database: &Database, request: &ShareRenderRequest) -> AppResult<ShareData> {
    let overview = database.overview(&request.range, IndexStatus::default())?;
    let session_id = matches!(request.template_id.as_str(), "session-recap")
        .then(|| {
            request
                .session_id
                .as_ref()
                .filter(|value| !value.trim().is_empty())
                .cloned()
                .or_else(|| {
                    overview
                        .recent_sessions
                        .iter()
                        .find(|item| {
                            item.files_touched > 0 || item.lines_added + item.lines_deleted > 0
                        })
                        .or_else(|| overview.recent_sessions.first())
                        .map(|item| item.id.clone())
                })
        })
        .flatten();
    let mut session = session_id
        .map(|id| database.session_detail(&id))
        .transpose()?;
    if let Some(detail) = session.as_mut() {
        detail.summary.title = crate::privacy::clean_display_title(&detail.summary.title);
    }
    let comparison = database.comparison(&request.range)?;
    let vcti = (request.template_id == "vcti-card")
        .then(|| database.vcti_profile(&request.range))
        .transpose()?;
    let phrases = (request.template_id == "catchphrases")
        .then(|| database.phrase_cloud(&request.range))
        .transpose()?;
    Ok(ShareData {
        overview,
        comparison,
        session,
        vcti,
        phrases,
    })
}

fn validate_request(request: &ShareRenderRequest) -> AppResult<()> {
    if ![
        "usage-overview",
        "developer-wrapped",
        "agent-comparison",
        "session-recap",
        "vcti-card",
        "catchphrases",
    ]
    .contains(&request.template_id.as_str())
    {
        return Err(AppError::InvalidRequest("unknown share template".into()));
    }
    dimensions(&request.aspect_ratio)?;
    if !["light", "dark"].contains(&request.theme.as_str()) {
        return Err(AppError::InvalidRequest("unknown share theme".into()));
    }
    if !["en-US", "zh-CN"].contains(&loc::normalize_locale(&request.locale)) {
        return Err(AppError::InvalidRequest("unknown share locale".into()));
    }
    Ok(())
}

fn dimensions(aspect_ratio: &str) -> AppResult<(u32, u32)> {
    match aspect_ratio {
        "1:1" => Ok((2400, 2400)),
        "2:3" => Ok((1920, 2880)),
        "3:2" => Ok((2880, 1920)),
        "3:4" => Ok((2160, 2880)),
        "4:3" => Ok((2400, 1800)),
        "4:5" => Ok((2160, 2700)),
        "16:9" => Ok((2560, 1440)),
        "9:16" => Ok((1440, 2560)),
        _ => Err(AppError::InvalidRequest("unsupported aspect ratio".into())),
    }
}

fn render(request: &ShareRenderRequest, data: &ShareData, width: u32, height: u32) -> String {
    let palette = Palette::for_theme(&request.theme);
    let mut svg = String::with_capacity(28_000);
    let title_key = format!("template.{}", request.template_id);
    let accessible_title = if request.title.trim().is_empty() {
        loc::text(&request.locale, &title_key)
    } else {
        request.title.trim()
    };
    write!(
        svg,
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{width}\" height=\"{height}\" viewBox=\"0 0 {width} {height}\" role=\"img\"><title>{}</title><style>text{{font-family:'SF Pro Display','SF Pro Text','PingFang SC','Helvetica Neue',sans-serif;font-kerning:normal;text-rendering:geometricPrecision}}.d,.n{{font-family:'SF Pro Display','PingFang SC','SF Pro Text','Helvetica Neue',sans-serif;font-variant-numeric:tabular-nums}}.d{{letter-spacing:-0.025em}}</style><defs><linearGradient id=\"tg-accent\" x1=\"0\" y1=\"0\" x2=\"1\" y2=\"1\"><stop offset=\"0\" stop-color=\"{}\"/><stop offset=\"1\" stop-color=\"{}\"/></linearGradient><linearGradient id=\"bento-hero\" x1=\"0\" y1=\"0\" x2=\"1\" y2=\"1\"><stop offset=\"0\" stop-color=\"{}\"/><stop offset=\"1\" stop-color=\"{}\"/></linearGradient></defs>",
        xml(accessible_title), palette.accent, palette.accent_alt, palette.tile, palette.tile
    )
    .ok();
    rect(
        &mut svg,
        0.0,
        0.0,
        width as f64,
        height as f64,
        0.0,
        palette.background,
        None,
    );
    ambient_shapes(&mut svg, request, width, height, palette);
    match request.template_id.as_str() {
        "usage-overview" => {
            render_usage_overview_card(&mut svg, request, data, width, height, palette)
        }
        "developer-wrapped" => {
            render_developer_wrapped_card(&mut svg, request, data, width, height, palette)
        }
        "agent-comparison" => {
            render_agent_comparison_card(&mut svg, request, data, width, height, palette)
        }
        "session-recap" => {
            render_session_recap_card(&mut svg, request, data, width, height, palette)
        }
        "vcti-card" => render_vcti_card(&mut svg, request, data, width, height, palette),
        "catchphrases" => render_catchphrases_card(&mut svg, request, data, width, height, palette),
        _ => {}
    }
    if request.show_brand {
        render_brand(&mut svg, request, width, height, palette);
    }
    svg.push_str("</svg>");
    svg
}

fn render_catchphrases_card(
    svg: &mut String,
    request: &ShareRenderRequest,
    data: &ShareData,
    width: u32,
    height: u32,
    palette: Palette,
) {
    let locale = loc::normalize_locale(&request.locale);
    let margin = if width > height { 92.0 } else { 78.0 };
    let center_x = width as f64 / 2.0;
    let header_y = 74.0;
    rect(
        svg,
        margin,
        header_y - 17.0,
        18.0,
        18.0,
        5.0,
        "#FF8358",
        None,
    );
    text(
        svg,
        margin + 34.0,
        header_y,
        36.0,
        720,
        palette.muted,
        loc::text(locale, "label.catchphrases"),
        None,
    );
    text(
        svg,
        width as f64 - margin,
        header_y,
        32.0,
        560,
        palette.muted,
        &loc::format_range(locale, &request.range),
        Some("end"),
    );

    let default_title = loc::text(locale, "template.catchphrases");
    let title = custom_or(&request.title, default_title);
    let title_height = text_block_display_centered(
        svg,
        center_x,
        154.0,
        width as f64 - margin * 2.0,
        if width > height { 76.0 } else { 82.0 },
        720,
        palette.text,
        title,
        2,
    );
    let subtitle = custom_or(&request.summary, loc::text(locale, "catchphrases.subtitle"));
    let subtitle_y = 154.0 + title_height + 10.0;
    let subtitle_height = text_block_centered(
        svg,
        center_x,
        subtitle_y,
        width as f64 - margin * 2.0,
        32.0,
        540,
        palette.muted,
        subtitle,
        2,
    );
    let panel_top = (subtitle_y + subtitle_height + 30.0).max(300.0);

    let Some(champion) = data
        .phrases
        .as_ref()
        .and_then(|phrases| phrases.agents.items.first())
    else {
        render_catchphrase_empty(svg, locale, width, height, margin, panel_top, palette);
        return;
    };
    let landscape = width as f64 / height as f64 > 1.18;
    let panel_bottom = height as f64 - 220.0;
    let panel_width = width as f64 - margin * 2.0;
    let panel_height = panel_bottom - panel_top;
    let agent = champion.dominant_agent.as_deref().unwrap_or("unknown");
    let (badge_background, badge_foreground) = phrase_agent_style(agent, palette.dark);
    let agent = champion.dominant_agent.as_deref().map(agent_label);
    let source = match (&champion.dominant_model, agent) {
        (Some(model), Some(agent)) if !model.is_empty() => format!("{model} · {agent}"),
        (Some(model), _) if !model.is_empty() => model.clone(),
        (_, Some(agent)) => agent,
        _ => loc::text(locale, "catchphrases.unknown-source").to_string(),
    };
    let repeats = format!("{}×", loc::format_number(locale, champion.occurrences));
    let sessions = if locale == "zh-CN" {
        format!(
            "横跨 {}",
            loc::format_sessions(locale, champion.session_count)
        )
    } else {
        format!(
            "across {}",
            loc::format_sessions(locale, champion.session_count)
        )
    };
    panel(svg, margin, panel_top, panel_width, panel_height, palette);
    let inner_x = margin + if landscape { 64.0 } else { 58.0 };
    let inner_width = panel_width - if landscape { 128.0 } else { 116.0 };
    let phrase_length = champion.phrase.chars().count();
    let phrase_font = if phrase_length <= 6 {
        if landscape { 180.0 } else { 210.0 }
    } else if phrase_length <= 12 {
        if landscape { 145.0 } else { 170.0 }
    } else if landscape {
        110.0
    } else {
        128.0
    };
    let quoted_phrase = format!("“{}”", champion.phrase);
    if landscape {
        let divider_x = inner_x + inner_width * 0.64;
        let left_center = inner_x + (divider_x - inner_x) / 2.0;
        let right_center = divider_x + (inner_x + inner_width - divider_x) / 2.0;
        let top_y = panel_top + 72.0;
        text(
            svg,
            left_center,
            top_y,
            31.0,
            720,
            palette.muted,
            loc::text(locale, "catchphrases.champion"),
            Some("middle"),
        );
        let badge_width = (measure_text(&source, 29.0) + 44.0).min((divider_x - inner_x) * 0.78);
        rect(
            svg,
            left_center - badge_width / 2.0,
            top_y + 28.0,
            badge_width,
            56.0,
            18.0,
            badge_background,
            None,
        );
        text(
            svg,
            left_center,
            top_y + 66.0,
            29.0,
            680,
            badge_foreground,
            &source,
            Some("middle"),
        );
        let quote_y = panel_top + panel_height * 0.48;
        text_block_display_centered(
            svg,
            left_center,
            quote_y,
            (divider_x - inner_x) * 0.86,
            phrase_font,
            760,
            palette.text,
            &quoted_phrase,
            2,
        );
        line(
            svg,
            divider_x,
            panel_top + 64.0,
            divider_x,
            panel_top + panel_height - 64.0,
            palette.hairline,
            1.0,
            None,
        );
        text(
            svg,
            right_center,
            quote_y - 28.0,
            104.0,
            780,
            badge_foreground,
            &repeats,
            Some("middle"),
        );
        text(
            svg,
            right_center,
            quote_y + 42.0,
            35.0,
            680,
            palette.text,
            &sessions,
            Some("middle"),
        );
        text_block_centered(
            svg,
            right_center,
            quote_y + 122.0,
            (inner_x + inner_width - divider_x) * 0.84,
            39.0,
            600,
            palette.muted,
            loc::text(locale, "catchphrases.roast"),
            3,
        );
    } else {
        let panel_center = margin + panel_width / 2.0;
        let label_y = panel_top + (panel_height * 0.07).clamp(76.0, 138.0);
        text(
            svg,
            panel_center,
            label_y,
            32.0,
            720,
            palette.muted,
            loc::text(locale, "catchphrases.champion"),
            Some("middle"),
        );
        let badge_width = (measure_text(&source, 30.0) + 46.0).min(inner_width * 0.72);
        rect(
            svg,
            panel_center - badge_width / 2.0,
            label_y + 30.0,
            badge_width,
            58.0,
            19.0,
            badge_background,
            None,
        );
        text(
            svg,
            panel_center,
            label_y + 69.0,
            30.0,
            680,
            badge_foreground,
            &source,
            Some("middle"),
        );
        text_block_display_centered(
            svg,
            panel_center,
            panel_top + panel_height * 0.30,
            inner_width * 0.88,
            phrase_font,
            760,
            palette.text,
            &quoted_phrase,
            3,
        );
        let evidence_y = panel_top + panel_height * 0.62;
        line(
            svg,
            panel_center - inner_width * 0.34,
            evidence_y - 56.0,
            panel_center + inner_width * 0.34,
            evidence_y - 56.0,
            palette.hairline,
            1.0,
            None,
        );
        text(
            svg,
            panel_center,
            evidence_y,
            if width == height { 142.0 } else { 132.0 },
            780,
            badge_foreground,
            &repeats,
            Some("middle"),
        );
        text(
            svg,
            panel_center,
            evidence_y + 72.0,
            39.0,
            680,
            palette.text,
            &sessions,
            Some("middle"),
        );
        text_block_centered(
            svg,
            panel_center,
            panel_top + panel_height * 0.83,
            inner_width * 0.82,
            48.0,
            600,
            palette.muted,
            loc::text(locale, "catchphrases.roast"),
            2,
        );
    }
    text_block_centered(
        svg,
        center_x,
        height as f64 - 164.0,
        width as f64 - margin * 2.0,
        26.0,
        560,
        palette.muted,
        loc::text(locale, "catchphrases.method"),
        2,
    );
}

fn render_catchphrase_empty(
    svg: &mut String,
    locale: &str,
    width: u32,
    height: u32,
    margin: f64,
    panel_top: f64,
    palette: Palette,
) {
    let panel_height = height as f64 - panel_top - 220.0;
    panel(
        svg,
        margin,
        panel_top,
        width as f64 - margin * 2.0,
        panel_height,
        palette,
    );
    text(
        svg,
        width as f64 / 2.0,
        panel_top + panel_height / 2.0,
        38.0,
        650,
        palette.muted,
        loc::text(locale, "catchphrases.insufficient"),
        Some("middle"),
    );
}

fn phrase_agent_style(agent: &str, dark: bool) -> (&'static str, &'static str) {
    match (agent, dark) {
        ("codex", false) => ("#FFE0D5", "#9F3D21"),
        ("claude-code", false) => ("#F1DFD0", "#74452E"),
        ("kimi-code", false) => ("#DDEBFF", "#315F9D"),
        ("cursor", false) => ("#E5E0FF", "#5B4EB3"),
        ("openclaw", false) => ("#D9F2EA", "#26785F"),
        ("hermes", false) => ("#F7DDEB", "#99486F"),
        ("zcode", false) => ("#F8E7D0", "#9C5A20"),
        ("codex", true) => ("#5A2B22", "#FFB098"),
        ("claude-code", true) => ("#4B3429", "#E9B78E"),
        ("kimi-code", true) => ("#223D62", "#9AC5FF"),
        ("cursor", true) => ("#312A59", "#BDB4FF"),
        ("openclaw", true) => ("#203F37", "#82D8BC"),
        ("hermes", true) => ("#512B40", "#F4A2C9"),
        ("zcode", true) => ("#4A3521", "#F2B66D"),
        (_, false) => ("#E7E2EF", "#5F526F"),
        (_, true) => ("#342D3D", "#BFAFCF"),
    }
}

fn render_vcti_card(
    svg: &mut String,
    request: &ShareRenderRequest,
    data: &ShareData,
    width: u32,
    height: u32,
    palette: Palette,
) {
    let Some(profile) = data.vcti.as_ref() else {
        render_empty(svg, request, width, height, palette);
        return;
    };
    let Some(code) = profile.primary_type.as_deref() else {
        render_empty(svg, request, width, height, palette);
        return;
    };
    let locale = loc::normalize_locale(&request.locale);
    let landscape = width as f64 / height as f64 > 1.18;
    let margin = if landscape { 92.0 } else { 84.0 };
    let panel_y = 142.0;
    let panel_height = height as f64 - panel_y - 154.0;
    let panel_width = width as f64 - margin * 2.0;
    let guild = profile.guild.as_deref().unwrap_or("start");
    let accent = vcti_guild_color(guild, palette);
    let type_name = vcti_type_name(locale, code);
    let guild_name = vcti_guild_name(locale, guild);
    let fallback_tagline = vcti_type_tagline(locale, code);
    let tagline = custom_or(&request.summary, fallback_tagline);

    rect(svg, margin, 62.0, 18.0, 18.0, 5.0, accent, None);
    text(
        svg,
        margin + 34.0,
        82.0,
        30.0,
        720,
        palette.muted,
        loc::text(locale, "label.vcti-card"),
        None,
    );
    text(
        svg,
        width as f64 - margin,
        82.0,
        28.0,
        560,
        palette.muted,
        &vcti_range_caption(locale, &request.range),
        Some("end"),
    );
    panel(svg, margin, panel_y, panel_width, panel_height, palette);

    let avatar_size = if landscape {
        (panel_height * 0.48).min(panel_width * 0.27).max(260.0)
    } else {
        (panel_width * 0.36)
            .min(panel_height * 0.30)
            .clamp(300.0, 760.0)
    };
    let avatar_x = margin + 48.0;
    let avatar_y = panel_y + 62.0;
    render_vcti_avatar(
        svg,
        avatar_x,
        avatar_y,
        avatar_size,
        code,
        guild,
        accent,
        palette,
    );

    let identity_x = avatar_x + avatar_size + 62.0;
    let identity_width = width as f64 - margin - identity_x - 38.0;
    text(
        svg,
        identity_x,
        avatar_y + 34.0,
        30.0,
        690,
        accent,
        guild_name,
        None,
    );
    text(
        svg,
        identity_x,
        avatar_y + 146.0,
        if landscape { 112.0 } else { 96.0 },
        760,
        palette.text,
        code,
        Some("d"),
    );
    text_block_display(
        svg,
        identity_x,
        avatar_y + 228.0,
        identity_width,
        if landscape { 58.0 } else { 52.0 },
        700,
        palette.text,
        type_name,
        2,
    );
    text_block(
        svg,
        identity_x,
        avatar_y + 330.0,
        identity_width,
        if landscape { 30.0 } else { 28.0 },
        500,
        palette.muted,
        tagline,
        3,
    );
    let badge_y = avatar_y + avatar_size - 46.0;
    let mut badge_x = identity_x;
    for badge in profile.badges.iter().take(2) {
        let label = format!("{} · {}", badge.code, vcti_badge_name(locale, &badge.code));
        let badge_width = (measure_text(&label, 22.0) + 44.0).min(identity_width);
        rect(
            svg,
            badge_x,
            badge_y,
            badge_width,
            48.0,
            24.0,
            palette.accent_soft,
            None,
        );
        text(
            svg,
            badge_x + 22.0,
            badge_y + 32.0,
            22.0,
            680,
            accent,
            &label,
            None,
        );
        badge_x += badge_width + 14.0;
    }

    let scores_y = if landscape {
        panel_y + panel_height * 0.58
    } else {
        avatar_y + avatar_size + 82.0
    };
    let scores = profile.scores.iter().take(6).collect::<Vec<_>>();
    let gap = 18.0;
    let score_width = (panel_width - 96.0 - gap * 2.0) / 3.0;
    let score_height = if landscape {
        150.0
    } else {
        ((panel_y + panel_height - scores_y) * 0.12).clamp(164.0, 210.0)
    };
    for (index, score) in scores.iter().enumerate() {
        let column = index % 3;
        let row = index / 3;
        let x = margin + 48.0 + column as f64 * (score_width + gap);
        let y = scores_y + row as f64 * (score_height + gap);
        rect(
            svg,
            x,
            y,
            score_width,
            score_height,
            26.0,
            palette.tile,
            None,
        );
        text(
            svg,
            x + 24.0,
            y + 40.0,
            23.0,
            620,
            palette.muted,
            vcti_score_name(locale, &score.id),
            None,
        );
        text(
            svg,
            x + 24.0,
            y + 100.0,
            48.0,
            730,
            palette.text,
            &format!("{:.0}", score.value),
            Some("dn"),
        );
        let track_width = score_width - 132.0;
        rect(
            svg,
            x + 104.0,
            y + 78.0,
            track_width,
            14.0,
            7.0,
            palette.hairline,
            None,
        );
        rect(
            svg,
            x + 104.0,
            y + 78.0,
            track_width * (score.value / 100.0).clamp(0.0, 1.0),
            14.0,
            7.0,
            accent,
            None,
        );
        text(
            svg,
            x + 24.0,
            y + 132.0,
            18.0,
            520,
            palette.muted,
            if score.coverage < 0.5 {
                if locale == "zh-CN" {
                    "证据覆盖有限"
                } else {
                    "LIMITED COVERAGE"
                }
            } else if locale == "zh-CN" {
                "真实行为推断"
            } else {
                "BEHAVIOR-DERIVED"
            },
            None,
        );
    }

    let evidence_y = scores_y + score_height * 2.0 + gap + 34.0;
    if !landscape {
        let footer_y = panel_y + panel_height - 224.0;
        let evidence_end = render_vcti_identity_evidence(
            svg,
            locale,
            profile,
            margin + 48.0,
            evidence_y,
            panel_width - 96.0,
            false,
            accent,
            palette,
        );
        let mut content_end = evidence_end;
        if request.show_behavior_evidence {
            let structural = profile
                .evidence
                .iter()
                .filter(|item| item.structural)
                .take(2)
                .collect::<Vec<_>>();
            let structural_y = evidence_end + 26.0;
            if !structural.is_empty() && structural_y + 110.0 < footer_y {
                rect(
                    svg,
                    margin + 48.0,
                    structural_y,
                    panel_width - 96.0,
                    104.0,
                    22.0,
                    palette.accent_soft,
                    None,
                );
                text(
                    svg,
                    margin + 72.0,
                    structural_y + 36.0,
                    19.0,
                    700,
                    accent,
                    if locale == "zh-CN" {
                        "可选展示 · 提示词结构结论"
                    } else {
                        "OPTIONAL · PROMPT STRUCTURE"
                    },
                    None,
                );
                let joined = structural
                    .iter()
                    .map(|item| {
                        format!(
                            "{} {}",
                            vcti_evidence_name(locale, &item.id),
                            vcti_evidence_value(locale, item)
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("   ·   ");
                text(
                    svg,
                    margin + 72.0,
                    structural_y + 75.0,
                    25.0,
                    620,
                    palette.text,
                    &joined,
                    None,
                );
                content_end = structural_y + 104.0;
            }
        }
        let dimensions_y = content_end + 28.0;
        let dimensions_height = (footer_y - dimensions_y - 34.0).min(360.0);
        if dimensions_height >= 220.0 && profile.dimensions.len() >= 12 {
            rect(
                svg,
                margin + 48.0,
                dimensions_y,
                panel_width - 96.0,
                dimensions_height,
                24.0,
                palette.tile,
                None,
            );
            text(
                svg,
                margin + 72.0,
                dimensions_y + 42.0,
                20.0,
                700,
                palette.muted,
                if locale == "zh-CN" {
                    "18 项本地行为指标"
                } else {
                    "18 LOCAL BEHAVIOR METRICS"
                },
                None,
            );
            let chart_x = margin + 72.0;
            let chart_width = panel_width - 144.0;
            let slot_width = chart_width / profile.dimensions.len().min(18) as f64;
            let chart_y = dimensions_y + 66.0;
            let label_font_size = if locale == "zh-CN" { 13.0 } else { 12.0 };
            let label_band_height = if locale == "zh-CN" { 62.0 } else { 84.0 };
            let label_y = dimensions_y + dimensions_height - 18.0;
            let chart_bottom = label_y - label_band_height;
            let chart_height = (chart_bottom - chart_y).max(36.0);
            for (index, dimension) in profile.dimensions.iter().take(18).enumerate() {
                let bar_width = (slot_width * 0.38).clamp(12.0, 34.0);
                let bar_x = chart_x + slot_width * (index as f64 + 0.5) - bar_width / 2.0;
                rect(
                    svg,
                    bar_x,
                    chart_y,
                    bar_width,
                    chart_height,
                    bar_width / 2.0,
                    palette.hairline,
                    None,
                );
                render_vcti_dimension_value(
                    svg,
                    bar_x,
                    chart_y,
                    bar_width,
                    chart_height,
                    accent,
                    palette,
                    dimension,
                );
                rotated_text(
                    svg,
                    chart_x + slot_width * (index as f64 + 0.5) - 4.0,
                    label_y,
                    label_font_size,
                    620,
                    palette.muted,
                    vcti_dimension_name(locale, &dimension.id),
                    -50.0,
                    "start",
                );
            }
        }
        line(
            svg,
            margin + 48.0,
            footer_y,
            margin + panel_width - 48.0,
            footer_y,
            palette.hairline,
            2.0,
            None,
        );
        let secondary = profile
            .secondary_type
            .as_deref()
            .map(|secondary| {
                format!(
                    "{} {} · {}",
                    if locale == "zh-CN" {
                        "副人格"
                    } else {
                        "SECONDARY"
                    },
                    secondary,
                    vcti_type_name(locale, secondary)
                )
            })
            .unwrap_or_else(|| {
                if locale == "zh-CN" {
                    "人格仍在持续校准".into()
                } else {
                    "PROFILE STILL CALIBRATING".into()
                }
            });
        text(
            svg,
            margin + 48.0,
            footer_y + 57.0,
            30.0,
            690,
            palette.text,
            &secondary,
            None,
        );
        text(
            svg,
            margin + panel_width - 48.0,
            footer_y + 52.0,
            24.0,
            650,
            accent,
            &format!(
                "{} · {:.0}%",
                vcti_confidence_name(locale, &profile.confidence_label),
                profile.confidence
            ),
            Some("end"),
        );
        text(
            svg,
            margin + panel_width - 48.0,
            footer_y + 91.0,
            21.0,
            540,
            palette.muted,
            &format!(
                "{} {} · {} {}",
                loc::format_number(locale, profile.session_count),
                if locale == "zh-CN" {
                    "次会话"
                } else {
                    "SESSIONS"
                },
                loc::format_number(locale, profile.active_days),
                if locale == "zh-CN" {
                    "个活跃日"
                } else {
                    "ACTIVE DAYS"
                }
            ),
            Some("end"),
        );
        text(
            svg,
            margin + 48.0,
            footer_y + 154.0,
            20.0,
            520,
            palette.muted,
            if locale == "zh-CN" {
                "由本机可读 Coding Agent 行为生成 · 不上传 Prompt 或代码 · VibeMeter"
            } else {
                "Generated from readable local agent behavior · no prompts or code uploaded · VibeMeter"
            },
            None,
        );
    } else {
        render_vcti_identity_evidence(
            svg,
            locale,
            profile,
            identity_x,
            avatar_y + 370.0,
            identity_width,
            true,
            accent,
            palette,
        );
        let structural = request.show_behavior_evidence.then(|| {
            profile
                .evidence
                .iter()
                .filter(|item| item.structural)
                .take(2)
                .collect::<Vec<_>>()
        });
        if let Some(structural) = structural
            .filter(|items| !items.is_empty() && evidence_y + 80.0 < panel_y + panel_height)
        {
            text(
                svg,
                margin + 48.0,
                evidence_y,
                21.0,
                700,
                accent,
                if locale == "zh-CN" {
                    "可选展示 · 提示词结构结论"
                } else {
                    "OPTIONAL · PROMPT STRUCTURE"
                },
                None,
            );
            let joined = structural
                .iter()
                .map(|item| {
                    format!(
                        "{} {:.0}%",
                        vcti_evidence_name(locale, &item.id),
                        item.value
                    )
                })
                .collect::<Vec<_>>()
                .join("   ·   ");
            text_block(
                svg,
                margin + 48.0,
                evidence_y + 44.0,
                panel_width - 96.0,
                26.0,
                560,
                palette.text,
                &joined,
                2,
            );
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn render_vcti_identity_evidence(
    svg: &mut String,
    locale: &str,
    profile: &VctiProfile,
    x: f64,
    y: f64,
    width: f64,
    landscape: bool,
    accent: &str,
    palette: Palette,
) -> f64 {
    let summaries = vcti_identity_evidence_summaries(locale, profile);
    let columns = if landscape { 4 } else { 2 };
    let gap = if landscape { 12.0 } else { 18.0 };
    let card_width = (width - gap * (columns - 1) as f64) / columns as f64;
    let card_height = if landscape { 122.0 } else { 144.0 };
    let heading_size = if landscape { 18.0 } else { 24.0 };
    text(
        svg,
        x,
        y,
        heading_size,
        720,
        accent,
        if locale == "zh-CN" {
            "人格依据"
        } else {
            "IDENTITY EVIDENCE"
        },
        None,
    );
    let cards_y = y + if landscape { 20.0 } else { 30.0 };
    for (index, (label, value)) in summaries.iter().enumerate() {
        let column = index % columns;
        let row = index / columns;
        let card_x = x + column as f64 * (card_width + gap);
        let card_y = cards_y + row as f64 * (card_height + gap);
        rect(
            svg,
            card_x,
            card_y,
            card_width,
            card_height,
            if landscape { 18.0 } else { 24.0 },
            palette.tile,
            Some(palette.hairline),
        );
        text(
            svg,
            card_x + 20.0,
            card_y + 34.0,
            if landscape { 16.0 } else { 20.0 },
            700,
            accent,
            label,
            None,
        );
        text_block(
            svg,
            card_x + 20.0,
            card_y + 70.0,
            card_width - 40.0,
            if landscape { 17.0 } else { 21.0 },
            560,
            palette.text,
            value,
            2,
        );
    }
    let rows = summaries.len().div_ceil(columns);
    cards_y + rows as f64 * card_height + rows.saturating_sub(1) as f64 * gap
}

fn vcti_identity_evidence_summaries(
    locale: &str,
    profile: &VctiProfile,
) -> Vec<(&'static str, String)> {
    let zh = locale == "zh-CN";
    let identity = &profile.identity_evidence;
    let rhythm = if identity.rhythm.work_periods_available {
        let periods = identity
            .rhythm
            .work_periods
            .iter()
            .filter(|period| period.sessions > 0)
            .map(|period| {
                format!(
                    "{} {}",
                    vcti_period_name(locale, &period.id),
                    vcti_count(locale, period.sessions as f64)
                )
            })
            .collect::<Vec<_>>()
            .join(" · ");
        if periods.is_empty() {
            if zh {
                "该范围内没有活动".into()
            } else {
                "No activity in this range".into()
            }
        } else {
            periods
        }
    } else {
        vcti_not_recorded(locale).into()
    };
    let collaboration = format!(
        "{} {} · {} {}",
        "Subagent",
        vcti_optional_count(locale, &identity.collaboration.subagent_starts),
        if zh { "并行" } else { "Parallel" },
        vcti_optional_count(locale, &identity.collaboration.parallel_batches),
    );
    let detail = format!(
        "{} {} · Skill {}",
        if zh { "工具" } else { "Tools" },
        vcti_optional_categories(locale, &identity.detail_diversity.tool_categories),
        vcti_optional_categories(locale, &identity.detail_diversity.explicit_skills),
    );
    let process = format!(
        "{} {} · {} {} · {} {}",
        if zh { "错误" } else { "Errors" },
        vcti_optional_count(locale, &identity.process_variation.errors),
        if zh { "重试" } else { "Retries" },
        vcti_optional_count(locale, &identity.process_variation.retries),
        if zh { "回滚" } else { "Rollbacks" },
        vcti_optional_count(locale, &identity.process_variation.rollbacks),
    );
    vec![
        (if zh { "工作节奏" } else { "WORK RHYTHM" }, rhythm),
        (if zh { "协作方式" } else { "COLLABORATION" }, collaboration),
        (
            if zh {
                "工具与 Skill"
            } else {
                "TOOLS & SKILL"
            },
            detail,
        ),
        (if zh { "过程记录" } else { "PROCESS RECORD" }, process),
    ]
}

fn vcti_optional_count(locale: &str, metric: &VctiOptionalMetric) -> String {
    if metric.available {
        vcti_count(locale, metric.value.unwrap_or(0.0))
    } else {
        vcti_not_recorded(locale).into()
    }
}

fn vcti_optional_categories(locale: &str, metric: &VctiOptionalMetric) -> String {
    if !metric.available {
        return vcti_not_recorded(locale).into();
    }
    let value = format!("{:.0}", metric.value.unwrap_or(0.0));
    if locale == "zh-CN" {
        format!("{value} 类")
    } else {
        format!("{value} categories")
    }
}

fn vcti_count(locale: &str, value: f64) -> String {
    let value = format!("{value:.0}");
    if locale == "zh-CN" {
        format!("{value} 次")
    } else {
        value
    }
}

fn vcti_not_recorded(locale: &str) -> &'static str {
    if locale == "zh-CN" {
        "未记录"
    } else {
        "Not recorded"
    }
}

fn vcti_period_name(locale: &str, period: &str) -> &'static str {
    match (locale, period) {
        ("zh-CN", "night") => "深夜",
        ("zh-CN", "morning") => "上午",
        ("zh-CN", "afternoon") => "下午",
        ("zh-CN", "evening") => "晚上",
        (_, "night") => "Night",
        (_, "morning") => "Morning",
        (_, "afternoon") => "Afternoon",
        (_, "evening") => "Evening",
        ("zh-CN", _) => "其他时段",
        _ => "Other",
    }
}

#[allow(clippy::too_many_arguments)]
fn render_vcti_dimension_value(
    svg: &mut String,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    accent: &str,
    palette: Palette,
    dimension: &crate::models::VctiScore,
) {
    if dimension.coverage > 0.0 {
        let fill_height = height * (dimension.value / 100.0).clamp(0.0, 1.0);
        rect(
            svg,
            x,
            y + height - fill_height,
            width,
            fill_height,
            width / 2.0,
            accent,
            None,
        );
    } else {
        let marker_y = y + height - 10.0;
        line(
            svg,
            x + 3.0,
            marker_y,
            x + width - 3.0,
            marker_y,
            palette.muted,
            2.0,
            None,
        );
    }
}

fn vcti_range_caption(locale: &str, range: &str) -> String {
    let label = loc::format_range(locale, range);
    if loc::normalize_locale(locale) == "zh-CN" {
        format!("{label} · 本机推断")
    } else {
        format!("{} · LOCAL INFERENCE", label.to_uppercase())
    }
}

#[allow(clippy::too_many_arguments)]
fn render_vcti_avatar(
    svg: &mut String,
    x: f64,
    y: f64,
    size: f64,
    code: &str,
    guild: &str,
    accent: &str,
    palette: Palette,
) {
    let clip_id = "vcti-avatar-clip";
    write!(
        svg,
        "<defs><clipPath id=\"{clip_id}\"><rect x=\"{x:.1}\" y=\"{y:.1}\" width=\"{size:.1}\" height=\"{size:.1}\" rx=\"{radius:.1}\"/></clipPath></defs>",
        radius = size * 0.12,
    )
    .ok();
    rect(
        svg,
        x,
        y,
        size,
        size,
        size * 0.12,
        if palette.dark { "#111318" } else { "#F2EEE5" },
        None,
    );
    let zoom = 1.2;
    write!(
        svg,
        "<g clip-path=\"url(#{clip_id})\"><image href=\"{}\" x=\"{:.1}\" y=\"{:.1}\" width=\"{:.1}\" height=\"{:.1}\" preserveAspectRatio=\"none\"/></g>",
        vcti_avatar_data_uri(code, guild),
        x - size * (zoom - 1.0) / 2.0,
        y - size * (zoom - 1.0) / 2.0,
        size * zoom,
        size * zoom,
    )
    .ok();
    rect(svg, x, y, size, size, size * 0.12, "none", Some(accent));
    let unit = size / 100.0;
    let code_width = (measure_text(code, 6.3 * unit) + 9.0 * unit).max(18.0 * unit);
    rect(
        svg,
        x + size - code_width - 5.0 * unit,
        y + size - 14.0 * unit,
        code_width,
        10.0 * unit,
        3.0 * unit,
        accent,
        None,
    );
    text(
        svg,
        x + size - 9.0 * unit,
        y + size - 6.5 * unit,
        6.3 * unit,
        760,
        "#FFFFFF",
        code,
        Some("end"),
    );
}

fn vcti_avatar_data_uri(code: &str, guild: &str) -> String {
    let fallback = match guild {
        "agent" => "BOSS",
        "quality" => "TEST",
        "debug" => "DEBUG",
        "delivery" => "SHIP",
        "tools" => "BUDDY",
        _ => "VIBE",
    };
    let bytes: &'static [u8] = match if vcti_type_known(code) {
        code
    } else {
        fallback
    } {
        "VIBE" => include_bytes!("../../src/assets/vcti/types-v2/VIBE.webp"),
        "SPEC" => include_bytes!("../../src/assets/vcti/types-v2/SPEC.webp"),
        "HACK" => include_bytes!("../../src/assets/vcti/types-v2/HACK.webp"),
        "MIX" => include_bytes!("../../src/assets/vcti/types-v2/MIX.webp"),
        "YOLO" => include_bytes!("../../src/assets/vcti/types-v2/YOLO.webp"),
        "LOOP" => include_bytes!("../../src/assets/vcti/types-v2/LOOP.webp"),
        "BOSS" => include_bytes!("../../src/assets/vcti/types-v2/BOSS.webp"),
        "SWARM" => include_bytes!("../../src/assets/vcti/types-v2/SWARM.webp"),
        "DIFF" => include_bytes!("../../src/assets/vcti/types-v2/DIFF.webp"),
        "TEST" => include_bytes!("../../src/assets/vcti/types-v2/TEST.webp"),
        "DOCS" => include_bytes!("../../src/assets/vcti/types-v2/DOCS.webp"),
        "UNDO" => include_bytes!("../../src/assets/vcti/types-v2/UNDO.webp"),
        "DEBUG" => include_bytes!("../../src/assets/vcti/types-v2/DEBUG.webp"),
        "PATCH" => include_bytes!("../../src/assets/vcti/types-v2/PATCH.webp"),
        "STACK" => include_bytes!("../../src/assets/vcti/types-v2/STACK.webp"),
        "AUTO" => include_bytes!("../../src/assets/vcti/types-v2/AUTO.webp"),
        "SHIP" => include_bytes!("../../src/assets/vcti/types-v2/SHIP.webp"),
        "RUSH" => include_bytes!("../../src/assets/vcti/types-v2/RUSH.webp"),
        "MVP" => include_bytes!("../../src/assets/vcti/types-v2/MVP.webp"),
        "DETAIL" => include_bytes!("../../src/assets/vcti/types-v2/DETAIL.webp"),
        "FORK" => include_bytes!("../../src/assets/vcti/types-v2/FORK.webp"),
        "TOKEN" => include_bytes!("../../src/assets/vcti/types-v2/TOKEN.webp"),
        "CACHE" => include_bytes!("../../src/assets/vcti/types-v2/CACHE.webp"),
        "BUDDY" => include_bytes!("../../src/assets/vcti/types-v2/BUDDY.webp"),
        _ => include_bytes!("../../src/assets/vcti/types-v2/VIBE.webp"),
    };
    format!("data:image/webp;base64,{}", BASE64_STANDARD.encode(bytes))
}

fn vcti_type_known(code: &str) -> bool {
    matches!(
        code,
        "VIBE"
            | "SPEC"
            | "HACK"
            | "MIX"
            | "YOLO"
            | "LOOP"
            | "BOSS"
            | "SWARM"
            | "DIFF"
            | "TEST"
            | "DOCS"
            | "UNDO"
            | "DEBUG"
            | "PATCH"
            | "STACK"
            | "AUTO"
            | "SHIP"
            | "RUSH"
            | "MVP"
            | "DETAIL"
            | "FORK"
            | "TOKEN"
            | "CACHE"
            | "BUDDY"
    )
}

fn vcti_guild_color(guild: &str, palette: Palette) -> &'static str {
    if palette.dark {
        return palette.accent;
    }
    match guild {
        "quality" => "#245CA6",
        "debug" => "#27354A",
        "tools" => "#3473A7",
        "agent" => "#DF5B3F",
        _ => "#EE7532",
    }
}

fn vcti_guild_name(locale: &str, guild: &str) -> &'static str {
    match (locale, guild) {
        ("zh-CN", "start") => "起手方式派",
        ("zh-CN", "agent") => "Agent 驾驭派",
        ("zh-CN", "quality") => "质量把关派",
        ("zh-CN", "debug") => "排障修复派",
        ("zh-CN", "delivery") => "交付节奏派",
        ("zh-CN", "tools") => "工具关系派",
        (_, "start") => "STARTING STYLE",
        (_, "agent") => "AGENT DIRECTION",
        (_, "quality") => "QUALITY CONTROL",
        (_, "debug") => "DEBUG & REPAIR",
        (_, "delivery") => "DELIVERY RHYTHM",
        _ => "TOOL RELATIONSHIP",
    }
}

fn vcti_type_name(locale: &str, code: &str) -> &'static str {
    match (locale, code) {
        ("zh-CN", "VIBE") => "感觉对了就开干",
        ("zh-CN", "SPEC") => "开工判官",
        ("zh-CN", "HACK") => "邪修玩家",
        ("zh-CN", "MIX") => "能拼就别造",
        ("zh-CN", "YOLO") => "全选就开冲",
        ("zh-CN", "LOOP") => "不行就重开",
        ("zh-CN", "BOSS") => "Agent 包工头",
        ("zh-CN", "SWARM") => "多开狂魔",
        ("zh-CN", "DIFF") => "逐行验尸官",
        ("zh-CN", "TEST") => "测试守门员",
        ("zh-CN", "DOCS") => "失忆预防针",
        ("zh-CN", "UNDO") => "后悔药批发商",
        ("zh-CN", "DEBUG") => "Bug 侦探",
        ("zh-CN", "PATCH") => "哪里漏补哪里",
        ("zh-CN", "STACK") => "大炮打蚊子",
        ("zh-CN", "AUTO") => "手动过敏症",
        ("zh-CN", "SHIP") => "发版战神",
        ("zh-CN", "RUSH") => "爆肝冲刺王",
        ("zh-CN", "MVP") => "先跑再说",
        ("zh-CN", "DETAIL") => "细节控",
        ("zh-CN", "FORK") => "见一个爱一个",
        ("zh-CN", "TOKEN") => "每句都算账",
        ("zh-CN", "CACHE") => "背景全塞给它",
        ("zh-CN", "BUDDY") => "搭子养成系",
        (_, "VIBE") => "Vibe Lead",
        (_, "SPEC") => "Spec Owner",
        (_, "HACK") => "Shortcut Hacker",
        (_, "MIX") => "Stack Stitcher",
        (_, "YOLO") => "All-in Operator",
        (_, "LOOP") => "One-more-version",
        (_, "BOSS") => "Agent Foreman",
        (_, "SWARM") => "Parallel Maniac",
        (_, "DIFF") => "Diff Supervisor",
        (_, "TEST") => "Test Gatekeeper",
        (_, "DOCS") => "Docs Diehard",
        (_, "UNDO") => "Rollback Master",
        (_, "DEBUG") => "Bug Detective",
        (_, "PATCH") => "Patch Hero",
        (_, "STACK") => "Infra Maximalist",
        (_, "AUTO") => "Automation Maniac",
        (_, "SHIP") => "Release Warrior",
        (_, "RUSH") => "Sprint Burner",
        (_, "MVP") => "Barebones Builder",
        (_, "DETAIL") => "Detail Controller",
        (_, "FORK") => "Tool Hopper",
        (_, "TOKEN") => "Token Accountant",
        (_, "CACHE") => "Context Hoarder",
        _ => "Cyber Partner",
    }
}

fn vcti_type_tagline(locale: &str, code: &str) -> &'static str {
    match (locale, code) {
        ("zh-CN", "VIBE") => "规格还没成形，第一版已经替你把感觉试出来了。",
        ("zh-CN", "SPEC") => "边界先钉死、验收先写清，Agent 想跑偏都没路。",
        ("zh-CN", "HACK") => "正门还在排队，你已经从侧门把结果拎回来了。",
        ("zh-CN", "MIX") => "轮子不用造，能把一地零件拼成车才是本事。",
        ("zh-CN", "YOLO") => "权限全开，验收随缘——先让 Agent 跑起来再说。",
        ("zh-CN", "LOOP") => "第一版只是开价，你总能把 Agent 磨到改口。",
        ("zh-CN", "BOSS") => "别人把 Agent 当助手，你已经给它们排班、派活、验收。",
        ("zh-CN", "SWARM") => "能并行绝不排队，Agent 不够就再开一队。",
        ("zh-CN", "DIFF") => "嘴上说“放手去做”，眼睛却没放过一行 Diff。",
        ("zh-CN", "TEST") => "没过测试的代码，在你这里连“能跑”都不算。",
        ("zh-CN", "DOCS") => "聊天会过期，文档才是你留给下一个 Agent 的记忆。",
        ("zh-CN", "UNDO") => "你敢把改动推到底，因为回滚路线早就铺好了。",
        ("zh-CN", "DEBUG") => "别人修报错，你追凶：非得揪出第一个倒下的环节。",
        ("zh-CN", "PATCH") => "先把血止住、服务拉起，根治排在下一张单。",
        ("zh-CN", "STACK") => "问题只要够小，你就敢给它配一整套基础设施。",
        ("zh-CN", "AUTO") => "手动重复一次叫工作，第二次就该判脚本接管。",
        ("zh-CN", "SHIP") => "讨论还没收尾，你的可用链接已经先到了。",
        ("zh-CN", "RUSH") => "平时留着油，冲刺一来就把进度条一脚踩满。",
        ("zh-CN", "MVP") => "不等精装交房，先让真实用户住进毛坯里。",
        ("zh-CN", "DETAIL") => "功能已经交付，你还在审最后两个像素。",
        ("zh-CN", "FORK") => "新工具一冒头，你的旧工具立刻被打入冷宫。",
        ("zh-CN", "TOKEN") => "Agent 每多想一步，你脑内的 Token 计价器就跳一下。",
        ("zh-CN", "CACHE") => "上下文宁可塞满，也不让 Agent 猜一个前提。",
        ("zh-CN", "BUDDY") => "工具会换，默契会攒；你把同一个 Agent 越用越顺手。",
        _ => "Your real agent workflow leaves a recognizable operating signature.",
    }
}

fn vcti_badge_name(locale: &str, code: &str) -> &'static str {
    match (locale, code) {
        ("zh-CN", "GUARD") => "守护者",
        ("zh-CN", "TURBO") => "加速党",
        ("zh-CN", "LIVE") => "活人感",
        ("zh-CN", "BUDGET") => "省流大师",
        ("zh-CN", "NIGHT") => "夜猫子",
        ("zh-CN", "MARATHON") => "长跑选手",
        ("zh-CN", "SOLO") => "单排玩家",
        ("zh-CN", "FINISH") => "收尾王",
        ("zh-CN", "STEADY") => "稳定器",
        (_, "GUARD") => "Guardian",
        (_, "TURBO") => "Accelerator",
        (_, "LIVE") => "Human Touch",
        (_, "BUDGET") => "Budget Master",
        (_, "NIGHT") => "Night Owl",
        (_, "MARATHON") => "Marathoner",
        (_, "SOLO") => "Solo Queue",
        (_, "FINISH") => "Finisher",
        _ => "Stabilizer",
    }
}

fn vcti_score_name(locale: &str, id: &str) -> &'static str {
    match (locale, id) {
        ("zh-CN", "startStructure") => "起手结构",
        ("zh-CN", "delegation") => "Agent 放权",
        ("zh-CN", "guardrail") => "工程守护",
        ("zh-CN", "debugDepth") => "排障深挖",
        ("zh-CN", "shipping") => "交付冲动",
        ("zh-CN", "toolNomad") => "工具游牧",
        (_, "startStructure") => "START STRUCTURE",
        (_, "delegation") => "DELEGATION",
        (_, "guardrail") => "GUARDRAILS",
        (_, "debugDepth") => "DEBUG DEPTH",
        (_, "shipping") => "SHIPPING",
        _ => "TOOL NOMAD",
    }
}

fn vcti_dimension_name(locale: &str, id: &str) -> &'static str {
    match (locale, id) {
        ("zh-CN", "requirementClarity") => "目标清晰",
        ("zh-CN", "exploration") => "探索倾向",
        ("zh-CN", "scopeDrift") => "范围漂移",
        ("zh-CN", "delegation") => "Agent 放权",
        ("zh-CN", "humanIntervention") => "人工介入",
        ("zh-CN", "parallelOrchestration") => "并行编排",
        ("zh-CN", "diffReview") => "Diff 审查",
        ("zh-CN", "automatedVerification") => "自动验证",
        ("zh-CN", "rollbackAwareness") => "回滚意识",
        ("zh-CN", "rootCause") => "根因深挖",
        ("zh-CN", "localFix") => "局部修复",
        ("zh-CN", "automation") => "自动化",
        ("zh-CN", "firstResultSpeed") => "首次响应",
        ("zh-CN", "iterationGranularity") => "迭代粒度",
        ("zh-CN", "shippingTendency") => "交付倾向",
        ("zh-CN", "toolSwitching") => "工具切换",
        ("zh-CN", "costRouting") => "成本路由",
        ("zh-CN", "contextReuse") => "上下文复用",
        (_, "requirementClarity") => "Clarity",
        (_, "exploration") => "Exploration",
        (_, "scopeDrift") => "Scope drift",
        (_, "delegation") => "Delegation",
        (_, "humanIntervention") => "Intervention",
        (_, "parallelOrchestration") => "Parallel",
        (_, "diffReview") => "Diff review",
        (_, "automatedVerification") => "Verification",
        (_, "rollbackAwareness") => "Rollback",
        (_, "rootCause") => "Root cause",
        (_, "localFix") => "Local fix",
        (_, "automation") => "Automation",
        (_, "firstResultSpeed") => "First result",
        (_, "iterationGranularity") => "Iteration",
        (_, "shippingTendency") => "Shipping",
        (_, "toolSwitching") => "Tool switch",
        (_, "costRouting") => "Cost route",
        _ => "Context reuse",
    }
}

fn vcti_evidence_name(locale: &str, id: &str) -> &'static str {
    match (locale, id) {
        ("zh-CN", "structure") => "结构化 Prompt",
        ("zh-CN", "acceptance") => "验收条件",
        ("zh-CN", "scope") => "文件范围",
        ("zh-CN", "verification") => "自动化验证",
        ("zh-CN", "diff") => "Diff 审查",
        ("zh-CN", "completion") => "任务完成率",
        ("zh-CN", "subagents") => "子 Agent 启动",
        ("zh-CN", "rollbacks") => "回滚与恢复",
        ("zh-CN", "plans") => "规划事件",
        ("zh-CN", "duration") => "平均任务时长",
        ("zh-CN", "sessions") => "有效会话",
        ("zh-CN", "automation") => "自动化事件",
        ("zh-CN", "style") => "界面打磨",
        ("zh-CN", "context") => "上下文沉淀",
        (_, "structure") => "Structured prompts",
        (_, "acceptance") => "Acceptance criteria",
        (_, "scope") => "File scope",
        (_, "verification") => "Automated verification",
        (_, "diff") => "Diff review",
        (_, "completion") => "Task completion",
        (_, "subagents") => "Subagents started",
        (_, "rollbacks") => "Rollbacks & recovery",
        (_, "plans") => "Planning events",
        (_, "duration") => "Average task duration",
        (_, "sessions") => "Qualified sessions",
        (_, "automation") => "Automation events",
        (_, "style") => "UI polish",
        _ => "Context retention",
    }
}

fn vcti_evidence_value(locale: &str, item: &VctiEvidenceItem) -> String {
    match item.format.as_str() {
        "percent" => format!("{:.0}%", item.value),
        "duration" => {
            if locale == "zh-CN" {
                format!("{:.1} 分钟", item.value / 60.0)
            } else {
                format!("{:.1} MIN", item.value / 60.0)
            }
        }
        _ => loc::format_number(locale, item.value.round().max(0.0) as u64),
    }
}

fn vcti_confidence_name(locale: &str, label: &str) -> &'static str {
    match (locale, label) {
        ("zh-CN", "high") => "高度匹配",
        ("zh-CN", "clear") => "明显匹配",
        ("zh-CN", "preview") => "初步人格",
        ("zh-CN", _) => "数据积累中",
        (_, "high") => "HIGH MATCH",
        (_, "clear") => "CLEAR MATCH",
        (_, "preview") => "PREVIEW",
        _ => "COLLECTING",
    }
}

fn render_usage_overview_card(
    svg: &mut String,
    request: &ShareRenderRequest,
    data: &ShareData,
    width: u32,
    height: u32,
    palette: Palette,
) {
    if data.overview.totals.session_count == 0 {
        render_empty(svg, request, width, height, palette);
        return;
    }
    let margin = if width > height { 92.0 } else { 84.0 };
    let title = custom_or(
        &request.title,
        loc::text(&request.locale, "template.usage-overview"),
    );
    let summary = custom_or_owned(
        &request.summary,
        loc::usage_overview_summary(
            &request.locale,
            data.overview.totals.session_count,
            data.overview.totals.active_days,
        ),
    );
    let content_top = render_flow_header(svg, request, title, &summary, margin, width, palette);
    let panel_y = content_top + 18.0;
    let panel_height = height as f64 - panel_y - 170.0;
    let panel_width = width as f64 - margin * 2.0;
    let values = [
        (
            "tokens",
            loc::text(&request.locale, "metric.tokens"),
            loc::format_number(&request.locale, data.overview.totals.usage.total()),
        ),
        (
            "sessions",
            loc::text(&request.locale, "metric.sessions"),
            loc::format_number(&request.locale, data.overview.totals.session_count),
        ),
        (
            "duration",
            loc::text(&request.locale, "metric.duration"),
            loc::format_duration(&request.locale, data.overview.totals.active_seconds),
        ),
        (
            "activeDays",
            loc::text(&request.locale, "metric.active-days"),
            loc::format_number(&request.locale, data.overview.totals.active_days),
        ),
    ]
    .into_iter()
    .filter(|(id, _, _)| metric_visible(request, id))
    .collect::<Vec<_>>();
    let metrics_height = render_metric_mosaic(
        svg,
        &values,
        margin,
        panel_y,
        panel_width,
        width > height,
        palette,
    );
    let analysis_y = panel_y + metrics_height + 14.0;
    let analysis_height = panel_y + panel_height - analysis_y;
    if width > height {
        let gap = 14.0;
        let half = (panel_width - gap) / 2.0;
        bento_tile(
            svg,
            margin,
            analysis_y,
            half,
            analysis_height,
            palette.tile,
            palette,
        );
        render_distribution_bars(
            svg,
            request,
            loc::text(&request.locale, "metric.top-agent"),
            &data.overview.agents,
            margin + 26.0,
            analysis_y + 24.0,
            half - 52.0,
            analysis_height - 48.0,
            palette,
        );
        let chart_x = margin + half + gap;
        bento_tile(
            svg,
            chart_x,
            analysis_y,
            half,
            analysis_height,
            palette.tile,
            palette,
        );
        render_usage_trend(
            svg,
            request,
            &data.overview,
            chart_x + 26.0,
            analysis_y + 24.0,
            half - 52.0,
            analysis_height - 48.0,
            palette,
        );
    } else {
        let block_height =
            (96.0 + data.overview.agents.len().min(5) as f64 * 72.0).clamp(220.0, 420.0);
        bento_tile(
            svg,
            margin,
            analysis_y,
            panel_width,
            block_height,
            palette.tile,
            palette,
        );
        render_distribution_bars(
            svg,
            request,
            loc::text(&request.locale, "metric.top-agent"),
            &data.overview.agents,
            margin + 26.0,
            analysis_y + 24.0,
            panel_width - 52.0,
            block_height - 48.0,
            palette,
        );
        let trend_y = analysis_y + block_height + 14.0;
        let trend_height = (analysis_height - block_height - 14.0).max(260.0);
        bento_tile(
            svg,
            margin,
            trend_y,
            panel_width,
            trend_height,
            palette.tile,
            palette,
        );
        render_usage_trend(
            svg,
            request,
            &data.overview,
            margin + 26.0,
            trend_y + 24.0,
            panel_width - 52.0,
            trend_height - 48.0,
            palette,
        );
    }
}

fn render_developer_wrapped_card(
    svg: &mut String,
    request: &ShareRenderRequest,
    data: &ShareData,
    width: u32,
    height: u32,
    palette: Palette,
) {
    if data.overview.totals.session_count == 0 {
        render_empty(svg, request, width, height, palette);
        return;
    }
    let margin = if width > height { 92.0 } else { 84.0 };
    let title = custom_or(
        &request.title,
        loc::text(&request.locale, "template.developer-wrapped"),
    );
    let summary = custom_or_owned(
        &request.summary,
        loc::developer_wrapped_summary(
            &request.locale,
            data.overview.totals.usage.total(),
            data.overview.totals.active_days,
        ),
    );
    let content_top = render_flow_header(svg, request, title, &summary, margin, width, palette);
    let panel_y = content_top + 18.0;
    let panel_height = height as f64 - panel_y - 170.0;
    let panel_width = width as f64 - margin * 2.0;
    let x = margin;
    let hero_height = if width > height { 300.0 } else { 330.0 };
    bento_tile(
        svg,
        x,
        panel_y,
        panel_width,
        hero_height,
        "url(#bento-hero)",
        palette,
    );
    text(
        svg,
        x + 30.0,
        panel_y + 54.0,
        40.0,
        680,
        palette.muted,
        loc::text(&request.locale, "metric.tokens"),
        None,
    );
    text_block_display(
        svg,
        x + 30.0,
        panel_y + hero_height - 24.0,
        panel_width - 60.0,
        if width > height { 186.0 } else { 176.0 },
        720,
        palette.accent,
        &loc::format_number(&request.locale, data.overview.totals.usage.total()),
        1,
    );
    let top_agent = data
        .overview
        .agents
        .first()
        .map(|item| agent_label(&item.label))
        .unwrap_or_else(|| "—".into());
    let top_model = data
        .overview
        .models
        .first()
        .map(|item| item.label.clone())
        .unwrap_or_else(|| "—".into());
    let active_date = most_active_date(&data.overview).unwrap_or_else(|| "—".into());
    let highlights = [
        (loc::text(&request.locale, "metric.top-agent"), top_agent),
        (loc::text(&request.locale, "metric.top-model"), top_model),
        (
            loc::text(&request.locale, "metric.active-date"),
            active_date,
        ),
    ];
    let highlight_y = panel_y + hero_height + 14.0;
    let gap = 14.0;
    let card_width = (panel_width - gap * 2.0) / 3.0;
    for (index, (label, value)) in highlights.iter().enumerate() {
        let card_x = x + index as f64 * (card_width + gap);
        bento_tile(
            svg,
            card_x,
            highlight_y,
            card_width,
            190.0,
            bento_fill(index + 1, palette),
            palette,
        );
        text(
            svg,
            card_x + 24.0,
            highlight_y + 50.0,
            36.0,
            560,
            palette.muted,
            label,
            None,
        );
        text_block_display(
            svg,
            card_x + 24.0,
            highlight_y + 156.0,
            card_width - 48.0,
            68.0,
            680,
            palette.accent,
            value,
            1,
        );
    }
    let values = [
        (
            "sessions",
            loc::text(&request.locale, "metric.sessions"),
            loc::format_number(&request.locale, data.overview.totals.session_count),
        ),
        (
            "duration",
            loc::text(&request.locale, "metric.duration"),
            loc::format_duration(&request.locale, data.overview.totals.active_seconds),
        ),
        (
            "files",
            loc::text(&request.locale, "metric.files"),
            loc::format_number(&request.locale, data.overview.totals.files_touched),
        ),
        (
            "lines",
            loc::text(&request.locale, "metric.lines"),
            loc::format_number(
                &request.locale,
                data.overview.totals.lines_added + data.overview.totals.lines_deleted,
            ),
        ),
    ]
    .into_iter()
    .filter(|(id, _, _)| metric_visible(request, id))
    .collect::<Vec<_>>();
    let grid_top = highlight_y + 204.0;
    let grid_height = render_metric_grid(
        svg,
        &values,
        x,
        grid_top,
        panel_width,
        if width > height { 1 } else { 2 },
        if width > height { 180.0 } else { 202.0 },
        palette,
    );
    if width <= height {
        let chart_y = grid_top + grid_height + 14.0;
        let chart_height = (panel_y + panel_height - chart_y).max(280.0);
        bento_tile(
            svg,
            x,
            chart_y,
            panel_width,
            chart_height,
            palette.tile,
            palette,
        );
        render_usage_trend(
            svg,
            request,
            &data.overview,
            x + 26.0,
            chart_y + 24.0,
            panel_width - 52.0,
            chart_height - 48.0,
            palette,
        );
    }
}

fn render_agent_comparison_card(
    svg: &mut String,
    request: &ShareRenderRequest,
    data: &ShareData,
    width: u32,
    height: u32,
    palette: Palette,
) {
    let agents = data
        .comparison
        .iter()
        .filter(|item| item.group_kind == "agent")
        .take(4)
        .collect::<Vec<_>>();
    if agents.is_empty() {
        render_empty(svg, request, width, height, palette);
        return;
    }
    let margin = if width > height { 92.0 } else { 84.0 };
    let title = custom_or(
        &request.title,
        loc::text(&request.locale, "template.agent-comparison"),
    );
    let summary = custom_or_owned(
        &request.summary,
        loc::agent_comparison_summary(&request.locale, agents.len() as u64),
    );
    let content_top = render_flow_header(svg, request, title, &summary, margin, width, palette);
    let panel_width = width as f64 - margin * 2.0;
    let draw = |svg: &mut String, panel_y: f64| -> f64 {
        let gap = 14.0;
        let available = (height as f64 - panel_y - 170.0).max(520.0);
        let rest = agents.len().saturating_sub(1);
        let base_height = if width > height {
            (available * 0.98).clamp(700.0, 1_300.0)
        } else {
            (available * 0.92).clamp(1_000.0, 2_100.0)
        };
        let minimum_side_height = if width > height { 230.0 } else { 270.0 };
        let minimum_cluster =
            rest as f64 * minimum_side_height + gap * rest.saturating_sub(1) as f64;
        let cluster_height = base_height.max(minimum_cluster).min(available);
        let hero_width = panel_width * if width > height { 0.44 } else { 0.57 };
        let side_x = margin + hero_width + gap;
        let side_width = panel_width - hero_width - gap;
        render_agent_comparison_tile(
            svg,
            request,
            agents[0],
            margin,
            panel_y,
            hero_width,
            cluster_height,
            true,
            palette,
        );
        if rest > 0 {
            let side_height = (cluster_height - gap * rest.saturating_sub(1) as f64) / rest as f64;
            for (index, item) in agents.iter().skip(1).enumerate() {
                render_agent_comparison_tile(
                    svg,
                    request,
                    item,
                    side_x,
                    panel_y + index as f64 * (side_height + gap),
                    side_width,
                    side_height,
                    false,
                    palette,
                );
            }
        }
        panel_y + cluster_height
    };
    framed_card(svg, content_top, height, 150.0, draw);
}

#[allow(clippy::too_many_arguments)]
fn render_agent_comparison_tile(
    svg: &mut String,
    request: &ShareRenderRequest,
    item: &ComparisonItem,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    hero: bool,
    palette: Palette,
) {
    bento_tile(svg, x, y, width, height, palette.tile, palette);
    let compact = !hero && height < 360.0;
    let pad = if hero {
        38.0
    } else if compact {
        24.0
    } else {
        30.0
    };
    let mark_size = if hero {
        138.0
    } else if compact {
        72.0
    } else {
        94.0
    };
    agent_mark(svg, x + pad, y + pad, &item.agent, mark_size, palette);
    text_block_display(
        svg,
        x + pad + mark_size + 22.0,
        y + pad + mark_size * 0.68,
        width - pad * 2.0 - mark_size - 22.0,
        if hero {
            58.0
        } else if compact {
            40.0
        } else {
            48.0
        },
        700,
        palette.text,
        &agent_label(&item.label),
        2,
    );
    if compact {
        let metric_y = y + height - 82.0;
        text(
            svg,
            x + pad,
            metric_y,
            30.0,
            680,
            palette.muted,
            loc::text(&request.locale, "metric.tokens"),
            None,
        );
        text(
            svg,
            x + width - pad,
            metric_y + 7.0,
            (height * 0.23).clamp(52.0, 72.0),
            720,
            palette.accent,
            &loc::format_number(&request.locale, item.usage.total()),
            Some("end"),
        );
    } else {
        let value_y = y + height * if hero { 0.60 } else { 0.62 };
        text(
            svg,
            x + pad,
            value_y - if hero { 202.0 } else { 142.0 },
            if hero { 44.0 } else { 38.0 },
            680,
            palette.muted,
            loc::text(&request.locale, "metric.tokens"),
            None,
        );
        text(
            svg,
            x + pad,
            value_y,
            if hero {
                (width * 0.20).clamp(132.0, 226.0)
            } else {
                (width * 0.17).clamp(96.0, 158.0)
            },
            720,
            palette.accent,
            &loc::format_number(&request.locale, item.usage.total()),
            Some("d"),
        );
    }
    let content_width = width - pad * 2.0;
    let detail = if hero || width > 460.0 {
        format!(
            "{} {} · {} · {} {}",
            loc::format_number(&request.locale, item.session_count),
            loc::text(&request.locale, "metric.sessions"),
            loc::format_duration(&request.locale, item.active_seconds),
            loc::format_number(&request.locale, item.files_touched),
            loc::text(&request.locale, "metric.files")
        )
    } else {
        format!(
            "{} {}",
            loc::format_number(&request.locale, item.session_count),
            loc::text(&request.locale, "metric.sessions")
        )
    };
    let detail_y = y + height - if compact { 28.0 } else { 50.0 };
    line(
        svg,
        x + pad,
        detail_y - if compact { 38.0 } else { 52.0 },
        x + width - pad,
        detail_y - if compact { 38.0 } else { 52.0 },
        palette.hairline,
        2.0,
        Some("0.9"),
    );
    text_block(
        svg,
        x + pad,
        detail_y,
        content_width,
        if hero {
            42.0
        } else if compact {
            28.0
        } else {
            38.0
        },
        620,
        palette.muted,
        &detail,
        1,
    );
}

fn render_session_recap_card(
    svg: &mut String,
    request: &ShareRenderRequest,
    data: &ShareData,
    width: u32,
    height: u32,
    palette: Palette,
) {
    let Some(session) = &data.session else {
        render_empty(svg, request, width, height, palette);
        return;
    };
    let margin = if width > height { 92.0 } else { 84.0 };
    let fallback_title = if session.summary.title.trim().is_empty() {
        loc::text(&request.locale, "template.session-recap").to_string()
    } else {
        session.summary.title.clone()
    };
    let title = custom_or(&request.title, &fallback_title);
    let summary = custom_or_owned(
        &request.summary,
        loc::session_recap_summary(
            &request.locale,
            &agent_label(&session.summary.agent),
            session.summary.usage.total(),
        ),
    );
    let content_top = render_flow_header(svg, request, title, &summary, margin, width, palette);
    let panel_y = content_top + 18.0;
    let panel_height = height as f64 - panel_y - 170.0;
    let panel_width = width as f64 - margin * 2.0;
    let hero_height = if width > height { 330.0 } else { 360.0 };
    bento_tile(
        svg,
        margin,
        panel_y,
        panel_width,
        hero_height,
        "url(#bento-hero)",
        palette,
    );
    let x = margin + 30.0;
    let inner_width = panel_width - 60.0;
    agent_mark(
        svg,
        x,
        panel_y + 42.0,
        &session.summary.agent,
        98.0,
        palette,
    );
    text(
        svg,
        x + 126.0,
        panel_y + 105.0,
        48.0,
        700,
        palette.text,
        &agent_label(&session.summary.agent),
        Some("d"),
    );
    if request.show_model {
        text(
            svg,
            x + 126.0,
            panel_y + 145.0,
            34.0,
            520,
            palette.muted,
            session.summary.model.as_deref().unwrap_or("—"),
            None,
        );
    }
    text(
        svg,
        x + inner_width,
        panel_y + 154.0,
        132.0,
        720,
        palette.accent,
        &loc::format_number(&request.locale, session.summary.usage.total()),
        Some("end"),
    );
    let usage = &session.summary.usage;
    let input = usage.input_tokens;
    let output = usage.output_tokens;
    let cache = usage.cache_read_tokens + usage.cache_write_tokens + usage.cache_write_1h_tokens;
    let total = (input + output + cache).max(1);
    let bar_y = panel_y + hero_height - 138.0;
    let input_width = inner_width * input as f64 / total as f64;
    let output_width = inner_width * output as f64 / total as f64;
    rect(
        svg,
        x,
        bar_y,
        inner_width,
        48.0,
        30.0,
        palette.hairline,
        None,
    );
    rect(svg, x, bar_y, input_width, 48.0, 24.0, palette.accent, None);
    rect(
        svg,
        x + input_width,
        bar_y,
        output_width,
        48.0,
        0.0,
        palette.positive,
        None,
    );
    rect(
        svg,
        x + input_width + output_width,
        bar_y,
        inner_width - input_width - output_width,
        48.0,
        30.0,
        palette.claude,
        None,
    );
    let legend = [
        (
            loc::text(&request.locale, "metric.input"),
            input,
            palette.accent,
        ),
        (
            loc::text(&request.locale, "metric.output"),
            output,
            palette.positive,
        ),
        (
            loc::text(&request.locale, "metric.cache"),
            cache,
            palette.claude,
        ),
    ];
    for (index, (label, value, color)) in legend.iter().enumerate() {
        let legend_x = x + index as f64 * inner_width / 3.0;
        circle(svg, legend_x + 9.0, bar_y + 92.0, 9.0, color, None);
        text(
            svg,
            legend_x + 32.0,
            bar_y + 101.0,
            36.0,
            560,
            palette.muted,
            &format!("{} {}", label, loc::format_number(&request.locale, *value)),
            None,
        );
    }
    let values = [
        (
            "duration",
            loc::text(&request.locale, "metric.duration"),
            loc::format_duration(&request.locale, session.summary.active_seconds),
        ),
        (
            "files",
            loc::text(&request.locale, "metric.files"),
            loc::format_number(&request.locale, session.summary.files_touched),
        ),
        (
            "lines",
            loc::text(&request.locale, "metric.lines"),
            loc::format_number(
                &request.locale,
                session.summary.lines_added + session.summary.lines_deleted,
            ),
        ),
        (
            "tools",
            loc::text(&request.locale, "metric.tool-calls"),
            loc::format_number(&request.locale, session.summary.tool_calls),
        ),
    ]
    .into_iter()
    .filter(|(id, _, _)| metric_visible(request, id))
    .collect::<Vec<_>>();
    let grid_top = panel_y + hero_height + 14.0;
    let grid_height = render_metric_grid(
        svg,
        &values,
        x,
        grid_top,
        inner_width,
        if width > height { 1 } else { 2 },
        if width > height { 174.0 } else { 204.0 },
        palette,
    );
    let tools_y = grid_top + grid_height + 14.0;
    let tools_height = if width > height {
        (panel_y + panel_height - tools_y).max(140.0)
    } else {
        410.0
    };
    bento_tile(
        svg,
        margin,
        tools_y,
        panel_width,
        tools_height,
        palette.tile,
        palette,
    );
    render_distribution_bars(
        svg,
        request,
        loc::text(&request.locale, "metric.tool-calls"),
        &session.tools,
        x,
        tools_y + 24.0,
        inner_width,
        tools_height - 48.0,
        palette,
    );
    if width <= height {
        let files_y = tools_y + tools_height + 14.0;
        let files_height = (panel_y + panel_height - files_y).max(250.0);
        bento_tile(
            svg,
            margin,
            files_y,
            panel_width,
            files_height,
            palette.tile,
            palette,
        );
        render_session_files(
            svg,
            request,
            session,
            x,
            files_y + 24.0,
            inner_width,
            palette,
        );
    }
}

fn render_session_files(
    svg: &mut String,
    request: &ShareRenderRequest,
    session: &SessionDetail,
    x: f64,
    y: f64,
    width: f64,
    palette: Palette,
) {
    if session.file_changes.is_empty() {
        return;
    }
    text(
        svg,
        x,
        y + 31.0,
        38.0,
        650,
        palette.muted,
        loc::text(&request.locale, "metric.files"),
        None,
    );
    for (index, file) in session.file_changes.iter().take(5).enumerate() {
        let row_y = y + 72.0 + index as f64 * 82.0;
        text(
            svg,
            x,
            row_y + 28.0,
            33.0,
            600,
            palette.text,
            &format!("{:02}", index + 1),
            Some("n"),
        );
        text_block_display(
            svg,
            x + 52.0,
            row_y + 28.0,
            width * 0.62,
            36.0,
            560,
            palette.text,
            &file.path,
            1,
        );
        text(
            svg,
            x + width,
            row_y + 28.0,
            33.0,
            560,
            palette.muted,
            &format!(
                "+{} / −{}",
                loc::format_number(&request.locale, file.lines_added),
                loc::format_number(&request.locale, file.lines_deleted)
            ),
            Some("end"),
        );
        line(
            svg,
            x + 52.0,
            row_y + 58.0,
            x + width,
            row_y + 58.0,
            palette.hairline,
            2.0,
            None,
        );
    }
}

#[allow(clippy::too_many_arguments)]
fn render_metric_grid(
    svg: &mut String,
    values: &[(&str, &str, String)],
    x: f64,
    y: f64,
    width: f64,
    rows: usize,
    tile_height: f64,
    palette: Palette,
) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    let gap = 14.0;
    let columns = values.len().div_ceil(rows.max(1)).max(1);
    let tile_width =
        ((width - gap * (columns.saturating_sub(1) as f64)) / columns as f64).max(120.0);
    let mut max_row = 0usize;
    for (index, (_, label, value)) in values.iter().enumerate() {
        let column = index % columns;
        let row = index / columns;
        max_row = max_row.max(row);
        let cell_x = x + column as f64 * (tile_width + gap);
        let cell_y = y + row as f64 * (tile_height + gap);
        bento_tile(
            svg,
            cell_x,
            cell_y,
            tile_width,
            tile_height,
            bento_fill(index, palette),
            palette,
        );
        rect(
            svg,
            cell_x + 24.0,
            cell_y + 26.0,
            16.0,
            16.0,
            5.0,
            palette.accent,
            None,
        );
        text(
            svg,
            cell_x + 52.0,
            cell_y + 49.0,
            36.0,
            600,
            palette.muted,
            label,
            None,
        );
        let value_size = if tile_height <= 180.0 { 76.0 } else { 94.0 };
        text_block_display(
            svg,
            cell_x + 24.0,
            cell_y + tile_height - 22.0,
            tile_width - 48.0,
            value_size,
            720,
            palette.accent,
            value,
            1,
        );
    }
    (max_row as f64 + 1.0) * tile_height + max_row as f64 * gap
}

fn render_metric_mosaic(
    svg: &mut String,
    values: &[(&str, &str, String)],
    x: f64,
    y: f64,
    width: f64,
    wide: bool,
    palette: Palette,
) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    let gap = 14.0;
    let hero_height = if wide { 300.0 } else { 360.0 };
    if values.len() == 1 {
        render_value_tile(svg, x, y, width, hero_height, &values[0], 0, true, palette);
        return hero_height;
    }

    let hero_width = width * if wide { 0.54 } else { 0.60 };
    let side_x = x + hero_width + gap;
    let side_width = width - hero_width - gap;
    render_value_tile(
        svg,
        x,
        y,
        hero_width,
        hero_height,
        &values[0],
        0,
        true,
        palette,
    );
    let side_count = values.len().saturating_sub(1).min(2);
    let side_height =
        (hero_height - gap * side_count.saturating_sub(1) as f64) / side_count.max(1) as f64;
    for (index, value) in values.iter().skip(1).take(2).enumerate() {
        render_value_tile(
            svg,
            side_x,
            y + index as f64 * (side_height + gap),
            side_width,
            side_height,
            value,
            index + 1,
            false,
            palette,
        );
    }
    if values.len() <= 3 {
        return hero_height;
    }
    let footer_y = y + hero_height + gap;
    let footer_height = if wide { 156.0 } else { 180.0 };
    render_value_tile(
        svg,
        x,
        footer_y,
        width,
        footer_height,
        &values[3],
        3,
        false,
        palette,
    );
    hero_height + gap + footer_height
}

#[allow(clippy::too_many_arguments)]
fn render_value_tile(
    svg: &mut String,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    value: &(&str, &str, String),
    index: usize,
    hero: bool,
    palette: Palette,
) {
    let fill = if hero {
        "url(#bento-hero)"
    } else {
        bento_fill(index, palette)
    };
    bento_tile(svg, x, y, width, height, fill, palette);
    let inline = !hero && width / height.max(1.0) > 4.2;
    rect(
        svg,
        x + 26.0,
        y + if inline { height * 0.5 - 8.0 } else { 27.0 },
        16.0,
        16.0,
        5.0,
        palette.accent,
        None,
    );
    text(
        svg,
        x + 54.0,
        y + if inline { height * 0.5 + 12.0 } else { 51.0 },
        if hero { 40.0 } else { 36.0 },
        650,
        palette.muted,
        value.1,
        None,
    );
    let preferred_value_size = if inline {
        (height * 0.54).clamp(76.0, 100.0)
    } else if hero {
        (height * 0.52).clamp(146.0, 184.0)
    } else {
        (height * 0.46).clamp(66.0, 98.0)
    };
    let value_width = if inline { width * 0.58 } else { width - 52.0 };
    let measured_value_width = measure_text(&value.2, preferred_value_size);
    let value_size = if measured_value_width > value_width {
        (preferred_value_size * value_width / measured_value_width).max(44.0)
    } else {
        preferred_value_size
    };
    text_block_display(
        svg,
        if inline { x + width * 0.39 } else { x + 26.0 },
        if inline {
            y + height * 0.72
        } else {
            y + height - if hero { 24.0 } else { 20.0 }
        },
        value_width,
        value_size,
        720,
        palette.accent,
        &value.2,
        1,
    );
}

#[allow(clippy::too_many_arguments)]
fn render_distribution_bars(
    svg: &mut String,
    request: &ShareRenderRequest,
    title: &str,
    items: &[crate::models::DistributionItem],
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    palette: Palette,
) {
    text(svg, x, y + 41.0, 40.0, 680, palette.muted, title, None);
    let visible = items.iter().take(5).collect::<Vec<_>>();
    let max = visible
        .first()
        .map(|item| item.value)
        .unwrap_or(1.0)
        .max(1.0);
    let row_height = ((height - 66.0).max(1.0) / visible.len().max(1) as f64).min(132.0);
    for (index, item) in visible.iter().enumerate() {
        let row_y = y + 66.0 + index as f64 * row_height;
        let label_size = (row_height * 0.34).clamp(30.0, 42.0);
        let bar_height = (row_height * 0.40).clamp(20.0, 44.0);
        let baseline = row_y + row_height * 0.62;
        text_block_display(
            svg,
            x,
            baseline,
            width * 0.27,
            label_size,
            600,
            palette.text,
            &agent_label(&item.label),
            1,
        );
        let bar_x = x + width * 0.30;
        let bar_width = width * 0.55;
        let bar_y = row_y + row_height * 0.48 - bar_height / 2.0;
        rect(
            svg,
            bar_x,
            bar_y,
            bar_width,
            bar_height,
            bar_height / 2.0,
            palette.hairline,
            None,
        );
        rect(
            svg,
            bar_x,
            bar_y,
            bar_width * item.value / max,
            bar_height,
            bar_height / 2.0,
            palette.accent,
            None,
        );
        text(
            svg,
            x + width,
            baseline,
            label_size,
            650,
            palette.accent,
            &loc::format_number(&request.locale, item.value.round() as u64),
            Some("end"),
        );
    }
}

#[allow(clippy::too_many_arguments)]
fn render_usage_trend(
    svg: &mut String,
    request: &ShareRenderRequest,
    overview: &OverviewResponse,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    palette: Palette,
) {
    text(
        svg,
        x,
        y + 41.0,
        40.0,
        650,
        palette.muted,
        loc::text(&request.locale, "metric.trend"),
        None,
    );
    let mut days = BTreeMap::<String, u64>::new();
    for point in &overview.daily {
        *days.entry(point.date.clone()).or_default() += point.usage.total();
    }
    let raw_values = days.values().map(|value| *value as f64).collect::<Vec<_>>();
    if raw_values.len() < 2 {
        text(
            svg,
            x,
            y + 96.0,
            32.0,
            520,
            palette.muted,
            loc::text(&request.locale, "empty.body"),
            None,
        );
        return;
    }
    let values = savitzky_golay_smooth(&raw_values);
    let chart_y = y + 66.0;
    let chart_height = (height - 78.0).max(110.0);
    let max = raw_values
        .iter()
        .chain(values.iter())
        .copied()
        .fold(1.0_f64, f64::max);
    let axis_width = (width * 0.095).clamp(82.0, 118.0);
    let plot_x = x + axis_width;
    let plot_width = (width - axis_width).max(120.0);
    let mut points = Vec::new();
    for (index, value) in values.iter().enumerate() {
        let px = plot_x + index as f64 * plot_width / (values.len() - 1) as f64;
        let py = chart_y + chart_height - chart_height * *value / max;
        points.push((px, py));
    }
    for (grid, tick) in trend_tick_values(max).iter().enumerate() {
        let gy = chart_y + grid as f64 * chart_height / 3.0;
        text(
            svg,
            plot_x - 14.0,
            gy + 8.0,
            (width * 0.024).clamp(22.0, 30.0),
            560,
            palette.muted,
            &loc::format_number(&request.locale, *tick),
            Some("end"),
        );
        line(
            svg,
            plot_x,
            gy,
            plot_x + plot_width,
            gy,
            palette.hairline,
            2.0,
            Some("0.75"),
        );
    }
    let path_data = interpolated_polyline_path(&points, 0.12);
    path(svg, &path_data, palette.accent, 14.0, "none");
    for (px, py) in points.iter().step_by((points.len() / 8).max(1)) {
        circle(svg, *px, *py, 12.0, palette.surface_strong, None);
        stroked_circle(svg, *px, *py, 12.0, palette.accent, 5.0, None, None, 0.0);
    }
}

fn trend_tick_values(max: f64) -> [u64; 4] {
    [
        max.round() as u64,
        (max * 2.0 / 3.0).round() as u64,
        (max / 3.0).round() as u64,
        0,
    ]
}

fn mirror_index(index: isize, length: usize) -> usize {
    if index < 0 {
        (-index) as usize
    } else if index >= length as isize {
        (2 * length as isize - index - 2) as usize
    } else {
        index as usize
    }
}

fn savitzky_golay_smooth(values: &[f64]) -> Vec<f64> {
    let kernel: &[f64] = if values.len() >= SAVITZKY_GOLAY_7.len() {
        &SAVITZKY_GOLAY_7
    } else if values.len() >= SAVITZKY_GOLAY_5.len() {
        &SAVITZKY_GOLAY_5
    } else {
        return values
            .iter()
            .map(|value| {
                if value.is_finite() {
                    value.max(0.0)
                } else {
                    0.0
                }
            })
            .collect();
    };
    let radius = (kernel.len() / 2) as isize;
    values
        .iter()
        .enumerate()
        .map(|(index, _)| {
            kernel
                .iter()
                .enumerate()
                .map(|(offset, coefficient)| {
                    let source_index =
                        mirror_index(index as isize + offset as isize - radius, values.len());
                    coefficient
                        * if values[source_index].is_finite() {
                            values[source_index]
                        } else {
                            0.0
                        }
                })
                .sum::<f64>()
                .max(0.0)
        })
        .collect()
}

fn interpolated_polyline_path(points: &[(f64, f64)], tension: f64) -> String {
    if points.is_empty() {
        return String::new();
    }
    if points.len() == 1 {
        return format!("M {:.1} {:.1}", points[0].0, points[0].1);
    }

    let min_x = points
        .iter()
        .map(|point| point.0)
        .fold(f64::INFINITY, f64::min);
    let max_x = points
        .iter()
        .map(|point| point.0)
        .fold(f64::NEG_INFINITY, f64::max);
    let min_y = points
        .iter()
        .map(|point| point.1)
        .fold(f64::INFINITY, f64::min);
    let max_y = points
        .iter()
        .map(|point| point.1)
        .fold(f64::NEG_INFINITY, f64::max);
    let mut controls = Vec::with_capacity((points.len() - 1) * 2);
    controls.push(points[0]);

    for index in 1..points.len() - 1 {
        let previous = points[index - 1];
        let current = points[index];
        let next = points[index + 1];
        let vector = (
            (next.0 - previous.0) * tension,
            (next.1 - previous.1) * tension,
        );
        let distance_before =
            ((current.0 - previous.0).powi(2) + (current.1 - previous.1).powi(2)).sqrt();
        let distance_after = ((current.0 - next.0).powi(2) + (current.1 - next.1).powi(2)).sqrt();
        let total_distance = (distance_before + distance_after).max(f64::EPSILON);
        let before_ratio = distance_before / total_distance;
        let after_ratio = distance_after / total_distance;
        controls.push((
            (current.0 - vector.0 * before_ratio).clamp(min_x, max_x),
            (current.1 - vector.1 * before_ratio).clamp(min_y, max_y),
        ));
        controls.push((
            (current.0 + vector.0 * after_ratio).clamp(min_x, max_x),
            (current.1 + vector.1 * after_ratio).clamp(min_y, max_y),
        ));
    }
    controls.push(*points.last().expect("non-empty points"));

    let mut path_data = format!("M {:.1} {:.1}", points[0].0, points[0].1);
    for (index, point) in points.iter().enumerate().skip(1) {
        let first = controls[(index - 1) * 2];
        let second = controls[(index - 1) * 2 + 1];
        path_data.push_str(&format!(
            " C {:.1} {:.1} {:.1} {:.1} {:.1} {:.1}",
            first.0, first.1, second.0, second.1, point.0, point.1
        ));
    }
    path_data
}

fn metric_visible(request: &ShareRenderRequest, id: &str) -> bool {
    request
        .metrics
        .iter()
        .find(|metric| metric.id == id)
        .is_none_or(|metric| metric.visible)
}

fn most_active_date(overview: &OverviewResponse) -> Option<String> {
    let mut days = BTreeMap::<String, u64>::new();
    for point in &overview.daily {
        *days.entry(point.date.clone()).or_default() += point.usage.total();
    }
    days.into_iter()
        .max_by_key(|(_, value)| *value)
        .map(|(date, _)| date)
}

fn render_flow_header(
    svg: &mut String,
    request: &ShareRenderRequest,
    title: &str,
    summary: &str,
    margin: f64,
    width: u32,
    palette: Palette,
) -> f64 {
    let eyebrow_key = match request.template_id.as_str() {
        "usage-overview" => "label.usage-overview",
        "developer-wrapped" => "label.developer-wrapped",
        "agent-comparison" => "label.agent-comparison",
        "session-recap" => "label.session-recap",
        _ => "label.real-data",
    };
    let eyebrow = loc::text(&request.locale, eyebrow_key);
    rect(svg, margin, 68.0, 17.0, 17.0, 5.0, palette.accent, None);
    text(
        svg,
        margin + 32.0,
        87.0,
        30.0,
        700,
        palette.muted,
        eyebrow,
        None,
    );
    text(
        svg,
        width as f64 - margin,
        87.0,
        30.0,
        520,
        palette.muted,
        &loc::format_range(&request.locale, &request.range),
        Some("end"),
    );
    let title_size = if width <= 1500 {
        80.0
    } else if width > 2450 {
        104.0
    } else {
        94.0
    };
    let title_height = text_block_display(
        svg,
        margin,
        174.0,
        width as f64 - margin * 2.0,
        title_size,
        700,
        palette.text,
        title,
        2,
    );
    let summary_y = 174.0 + title_height + 14.0;
    let summary_height = text_block(
        svg,
        margin,
        summary_y,
        width as f64 - margin * 2.0,
        40.0,
        500,
        palette.muted,
        summary,
        3,
    );
    summary_y + summary_height
}

/// Draws the bento cluster directly below the header. Ratio-specific renderers own
/// their density so sparse layouts cannot introduce a large, unexplained top gap.
fn framed_card<F>(svg: &mut String, content_top: f64, _height: u32, _bottom_reserve: f64, draw: F)
where
    F: Fn(&mut String, f64) -> f64,
{
    let _ = draw(svg, content_top + 18.0);
}

/// Baseline-aware text block: `top` is the top edge of the block; returns the
/// bottom edge (after descenders). Keeps flow layouts free of magic offsets.
fn header(
    svg: &mut String,
    request: &ShareRenderRequest,
    title: &str,
    summary: &str,
    margin: f64,
    width: u32,
    palette: Palette,
) {
    let eyebrow_key = match request.template_id.as_str() {
        "usage-overview" => "label.usage-overview",
        "developer-wrapped" => "label.developer-wrapped",
        "agent-comparison" => "label.agent-comparison",
        "session-recap" => "label.session-recap",
        _ => "label.real-data",
    };
    let eyebrow = loc::text(&request.locale, eyebrow_key);
    let chip_width = if loc::normalize_locale(&request.locale) == "zh-CN" {
        300.0
    } else {
        340.0
    };
    rect(
        svg,
        margin,
        82.0,
        chip_width,
        54.0,
        27.0,
        "url(#tg-accent)",
        None,
    );
    text(
        svg,
        margin + 26.0,
        117.0,
        19.0,
        700,
        "#FFFFFF",
        eyebrow,
        None,
    );
    text(
        svg,
        width as f64 - margin,
        116.0,
        21.0,
        520,
        palette.muted,
        &loc::format_range(&request.locale, &request.range),
        Some("end"),
    );
    text_block_display(
        svg,
        margin,
        258.0,
        width as f64 - margin * 2.0,
        if width <= 1500 {
            70.0
        } else if width > 2450 {
            94.0
        } else {
            84.0
        },
        700,
        palette.text,
        title,
        2,
    );
    text_block(
        svg,
        margin,
        382.0,
        width as f64 - margin * 2.0,
        29.0,
        440,
        palette.muted,
        summary,
        2,
    );
    line(
        svg,
        margin,
        438.0,
        width as f64 - margin,
        438.0,
        palette.hairline,
        2.0,
        Some("0.9"),
    );
}

fn render_empty(
    svg: &mut String,
    request: &ShareRenderRequest,
    width: u32,
    height: u32,
    palette: Palette,
) {
    let margin = if width > height { 92.0 } else { 84.0 };
    let title_key = format!("template.{}", request.template_id);
    let title = custom_or(&request.title, loc::text(&request.locale, &title_key));
    header(svg, request, title, "", margin, width, palette);
    let panel_y = if width > height { 430.0 } else { 540.0 };
    let panel_height = height as f64 - panel_y - 180.0;
    panel(
        svg,
        margin,
        panel_y,
        width as f64 - margin * 2.0,
        panel_height,
        palette,
    );
    let center_x = width as f64 / 2.0;
    let center_y = panel_y + panel_height / 2.0;
    circle(
        svg,
        center_x,
        center_y - 110.0,
        54.0,
        palette.accent_soft,
        None,
    );
    path(
        svg,
        &format!(
            "M {} {} L {} {} L {} {}",
            center_x - 22.0,
            center_y - 106.0,
            center_x - 4.0,
            center_y - 88.0,
            center_x + 28.0,
            center_y - 130.0
        ),
        palette.accent,
        8.0,
        "none",
    );
    text(
        svg,
        center_x,
        center_y + 30.0,
        42.0,
        650,
        palette.text,
        loc::text(&request.locale, "empty.title"),
        Some("middle"),
    );
    text(
        svg,
        center_x,
        center_y + 94.0,
        27.0,
        450,
        palette.muted,
        loc::text(&request.locale, "empty.body"),
        Some("middle"),
    );
}

fn render_brand(
    svg: &mut String,
    request: &ShareRenderRequest,
    width: u32,
    height: u32,
    palette: Palette,
) {
    let y = height as f64 - 78.0;
    let x = if width > height { 92.0 } else { 84.0 };
    let emphasized = request.template_id == "catchphrases";
    let icon_size = if emphasized { 54.0 } else { 46.0 };
    let icon_y = y - if emphasized { 40.0 } else { 34.0 };
    write!(
        svg,
        "<defs><clipPath id=\"vibemeter-brand-icon\"><rect x=\"{x:.1}\" y=\"{icon_y:.1}\" width=\"{icon_size:.1}\" height=\"{icon_size:.1}\" rx=\"11\"/></clipPath></defs><image href=\"{}\" x=\"{x:.1}\" y=\"{icon_y:.1}\" width=\"{icon_size:.1}\" height=\"{icon_size:.1}\" clip-path=\"url(#vibemeter-brand-icon)\" preserveAspectRatio=\"xMidYMid slice\"/>",
        vibemeter_brand_icon_data_uri(),
    )
    .ok();
    text(
        svg,
        x + if emphasized { 70.0 } else { 62.0 },
        y,
        if emphasized { 34.0 } else { 28.0 },
        650,
        palette.text,
        "VibeMeter",
        Some("d"),
    );
    let center = width as f64 / 2.0;
    let github_size = if emphasized { 29.0 } else { 24.0 };
    let github_x = center - if emphasized { 184.0 } else { 156.0 };
    let github_y = y - github_size + 4.0;
    write!(
        svg,
        "<a href=\"https://github.com/RangeKing/vibemeter\" aria-label=\"RangeKing/vibemeter on GitHub\"><g transform=\"translate({github_x:.1} {github_y:.1}) scale({:.4})\"><path fill=\"{}\" d=\"M12 2C6.48 2 2 6.58 2 12.24c0 4.52 2.87 8.35 6.84 9.71.5.1.68-.22.68-.49 0-.24-.01-1.05-.02-1.9-2.78.62-3.37-1.21-3.37-1.21-.45-1.18-1.11-1.49-1.11-1.49-.91-.64.07-.63.07-.63 1 .07 1.53 1.06 1.53 1.06.89 1.56 2.34 1.11 2.91.85.09-.66.35-1.11.63-1.37-2.22-.26-4.56-1.14-4.56-5.06 0-1.12.39-2.03 1.03-2.75-.1-.26-.45-1.3.1-2.71 0 0 .84-.28 2.75 1.05A9.32 9.32 0 0 1 12 6.11c.85 0 1.71.12 2.51.35 1.91-1.33 2.75-1.05 2.75-1.05.55 1.41.2 2.45.1 2.71.64.72 1.03 1.63 1.03 2.75 0 3.93-2.34 4.8-4.57 5.05.36.32.68.94.68 1.9 0 1.37-.01 2.47-.01 2.81 0 .27.18.59.69.49A10.25 10.25 0 0 0 22 12.24C22 6.58 17.52 2 12 2Z\"/></g>",
        github_size / 24.0,
        palette.text,
    )
    .ok();
    text(
        svg,
        center + 12.0,
        y,
        if emphasized { 29.0 } else { 24.0 },
        620,
        palette.text,
        "RangeKing/vibemeter",
        Some("middle"),
    );
    svg.push_str("</a>");
    let right = width as f64 - x;
    text(
        svg,
        right,
        y,
        if emphasized { 24.0 } else { 21.0 },
        460,
        palette.muted,
        loc::text(&request.locale, "brand.tagline"),
        Some("end"),
    );
}

fn vibemeter_brand_icon_data_uri() -> String {
    let bytes = include_bytes!("../icons/128x128.png");
    format!("data:image/png;base64,{}", BASE64_STANDARD.encode(bytes))
}

fn panel(svg: &mut String, x: f64, y: f64, width: f64, height: f64, palette: Palette) {
    rect(
        svg,
        x + 8.0,
        y + 14.0,
        width,
        height,
        36.0,
        if palette.dark { "#07090D" } else { "#D9D6CD" },
        None,
    );
    rect(
        svg,
        x,
        y,
        width,
        height,
        36.0,
        palette.surface,
        Some(palette.hairline),
    );
    rect(
        svg,
        x + 38.0,
        y,
        (width * 0.18).clamp(86.0, 230.0),
        7.0,
        3.5,
        "url(#tg-accent)",
        None,
    );
}

/// Apple product-spec bento tile: flat, neutral and consistent across the whole card.
/// Hierarchy comes from scale and the theme accent, never from a rainbow of fills.
fn bento_tile(
    svg: &mut String,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    fill: &str,
    palette: Palette,
) {
    rect(svg, x, y, width, height, 28.0, fill, Some(palette.hairline));
}

fn bento_fill(_index: usize, palette: Palette) -> &'static str {
    palette.tile
}

fn ambient_shapes(
    svg: &mut String,
    request: &ShareRenderRequest,
    width: u32,
    height: u32,
    palette: Palette,
) {
    let baseline = height as f64 - 132.0;
    line(
        svg,
        0.0,
        baseline,
        width as f64,
        baseline,
        palette.hairline,
        2.0,
        Some("0.7"),
    );
    let _ = request;
    let _ = width;
}

fn agent_mark(svg: &mut String, x: f64, y: f64, agent: &str, size: f64, palette: Palette) {
    let color = agent_color(agent, palette);
    rect(svg, x, y, size, size, size * 0.3, color, None);
    let letter = match agent {
        "claude-code" => "C",
        "deepseek-harness" => "D",
        "kimi-code" => "K",
        "zcode" => "Z",
        _ => "O",
    };
    text(
        svg,
        x + size / 2.0,
        y + size * 0.70,
        size * 0.46,
        700,
        "#FFFFFF",
        letter,
        Some("middle"),
    );
}

fn custom_or<'a>(custom: &'a str, fallback: &'a str) -> &'a str {
    if custom.trim().is_empty() {
        fallback
    } else {
        custom.trim()
    }
}

fn custom_or_owned(custom: &str, fallback: String) -> String {
    if custom.trim().is_empty() {
        fallback
    } else {
        custom.trim().to_string()
    }
}

fn agent_label(value: &str) -> String {
    match value {
        "claude-code" => "Claude Code".into(),
        "codex" => "Codex".into(),
        "kimi-code" => "Kimi Code".into(),
        "zcode" => "ZCode".into(),
        _ => value.into(),
    }
}

fn agent_color(value: &str, palette: Palette) -> &'static str {
    match value {
        "claude-code" => palette.claude,
        "kimi-code" => palette.kimi,
        "zcode" => palette.zcode,
        _ => palette.codex,
    }
}

#[allow(clippy::too_many_arguments)]
fn rect(
    svg: &mut String,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    radius: f64,
    fill: &str,
    stroke: Option<&str>,
) {
    write!(svg, "<rect x=\"{x:.1}\" y=\"{y:.1}\" width=\"{width:.1}\" height=\"{height:.1}\" rx=\"{radius:.1}\" fill=\"{fill}\"").ok();
    if let Some(stroke) = stroke {
        write!(svg, " stroke=\"{stroke}\" stroke-width=\"2\"").ok();
    }
    svg.push_str("/>");
}

#[allow(clippy::too_many_arguments)]
fn text(
    svg: &mut String,
    x: f64,
    y: f64,
    size: f64,
    weight: u16,
    fill: &str,
    value: &str,
    anchor_or_class: Option<&str>,
) {
    let (anchor, class) = match anchor_or_class {
        Some("end") => ("end", ""),
        Some("middle") => ("middle", ""),
        Some("d-middle") => ("middle", "d"),
        Some("n") => ("start", "n"),
        Some("d") => ("start", "d"),
        Some("dn") => ("start", "d n"),
        _ => ("start", ""),
    };
    write!(svg, "<text x=\"{x:.1}\" y=\"{y:.1}\" font-size=\"{size:.1}\" font-weight=\"{weight}\" fill=\"{fill}\" text-anchor=\"{anchor}\" class=\"{class}\">{}</text>", xml(value)).ok();
}

#[allow(clippy::too_many_arguments)]
fn rotated_text(
    svg: &mut String,
    x: f64,
    y: f64,
    size: f64,
    weight: u16,
    fill: &str,
    value: &str,
    angle: f64,
    anchor: &str,
) {
    write!(
        svg,
        "<text x=\"{x:.1}\" y=\"{y:.1}\" font-size=\"{size:.1}\" font-weight=\"{weight}\" fill=\"{fill}\" text-anchor=\"{anchor}\" transform=\"rotate({angle:.1} {x:.1} {y:.1})\">{}</text>",
        xml(value)
    )
    .ok();
}

#[allow(clippy::too_many_arguments)]
fn text_block(
    svg: &mut String,
    x: f64,
    y: f64,
    width: f64,
    size: f64,
    weight: u16,
    fill: &str,
    value: &str,
    max_lines: usize,
) -> f64 {
    let lines = wrap_text(value, width, size, max_lines);
    for (index, line_value) in lines.iter().enumerate() {
        text(
            svg,
            x,
            y + index as f64 * size * 1.24,
            size,
            weight,
            fill,
            line_value,
            None,
        );
    }
    lines.len().max(1) as f64 * size * 1.24
}

#[allow(clippy::too_many_arguments)]
fn text_block_display(
    svg: &mut String,
    x: f64,
    y: f64,
    width: f64,
    size: f64,
    weight: u16,
    fill: &str,
    value: &str,
    max_lines: usize,
) -> f64 {
    let lines = wrap_text(value, width, size, max_lines);
    for (index, line_value) in lines.iter().enumerate() {
        text(
            svg,
            x,
            y + index as f64 * size * 1.24,
            size,
            weight,
            fill,
            line_value,
            Some("d"),
        );
    }
    lines.len().max(1) as f64 * size * 1.24
}

#[allow(clippy::too_many_arguments)]
fn text_block_centered(
    svg: &mut String,
    center_x: f64,
    y: f64,
    width: f64,
    size: f64,
    weight: u16,
    fill: &str,
    value: &str,
    max_lines: usize,
) -> f64 {
    let lines = wrap_text(value, width, size, max_lines);
    for (index, line_value) in lines.iter().enumerate() {
        text(
            svg,
            center_x,
            y + index as f64 * size * 1.24,
            size,
            weight,
            fill,
            line_value,
            Some("middle"),
        );
    }
    lines.len().max(1) as f64 * size * 1.24
}

#[allow(clippy::too_many_arguments)]
fn text_block_display_centered(
    svg: &mut String,
    center_x: f64,
    y: f64,
    width: f64,
    size: f64,
    weight: u16,
    fill: &str,
    value: &str,
    max_lines: usize,
) -> f64 {
    let lines = wrap_text(value, width, size, max_lines);
    for (index, line_value) in lines.iter().enumerate() {
        text(
            svg,
            center_x,
            y + index as f64 * size * 1.24,
            size,
            weight,
            fill,
            line_value,
            Some("d-middle"),
        );
    }
    lines.len().max(1) as f64 * size * 1.24
}

fn char_advance(character: char, size: f64) -> f64 {
    if character.is_ascii_whitespace() {
        size * 0.28
    } else if character.is_ascii() {
        // Space Grotesk / Avenir-ish Latin average; digits are slightly wider.
        if character.is_ascii_digit() {
            size * 0.62
        } else if character.is_ascii_uppercase() {
            size * 0.66
        } else {
            size * 0.54
        }
    } else {
        // Full-width CJK and most non-Latin glyphs.
        size
    }
}

fn measure_text(value: &str, size: f64) -> f64 {
    value
        .chars()
        .map(|character| char_advance(character, size))
        .sum()
}

fn wrap_text(value: &str, width: f64, size: f64, max_lines: usize) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut ascii_word = String::new();
    let flush_ascii = |tokens: &mut Vec<String>, ascii_word: &mut String| {
        if !ascii_word.is_empty() {
            tokens.push(std::mem::take(ascii_word));
        }
    };
    for character in value.trim().chars() {
        if character.is_ascii() && !character.is_ascii_whitespace() {
            ascii_word.push(character);
        } else {
            flush_ascii(&mut tokens, &mut ascii_word);
            tokens.push(character.to_string());
        }
    }
    flush_ascii(&mut tokens, &mut ascii_word);

    let mut lines = Vec::new();
    let mut current = String::new();
    let mut current_width = 0.0;
    let mut truncated = false;
    for token in tokens {
        let token_width = measure_text(&token, size);
        if current_width + token_width > width && !current.trim().is_empty() {
            lines.push(current.trim().to_string());
            current.clear();
            current_width = 0.0;
            if lines.len() == max_lines {
                truncated = true;
                break;
            }
        }
        if current.is_empty() && token.trim().is_empty() {
            continue;
        }
        current.push_str(&token);
        current_width += token_width;
    }
    if lines.len() < max_lines && !current.trim().is_empty() {
        lines.push(current.trim().to_string());
    }
    if truncated && let Some(last) = lines.last_mut() {
        while measure_text(last, size) + measure_text("…", size) > width && last.chars().count() > 1
        {
            last.pop();
        }
        last.push('…');
    }
    if lines.is_empty() {
        lines.push(String::new());
    }
    lines
}

fn circle(svg: &mut String, cx: f64, cy: f64, radius: f64, fill: &str, opacity: Option<&str>) {
    write!(
        svg,
        "<circle cx=\"{cx:.1}\" cy=\"{cy:.1}\" r=\"{radius:.1}\" fill=\"{fill}\""
    )
    .ok();
    if let Some(opacity) = opacity {
        write!(svg, " opacity=\"{opacity}\"").ok();
    }
    svg.push_str("/>");
}

#[allow(clippy::too_many_arguments)]
fn stroked_circle(
    svg: &mut String,
    cx: f64,
    cy: f64,
    radius: f64,
    stroke: &str,
    width: f64,
    opacity: Option<&str>,
    dasharray: Option<&str>,
    dashoffset: f64,
) {
    write!(svg, "<circle cx=\"{cx:.1}\" cy=\"{cy:.1}\" r=\"{radius:.1}\" fill=\"none\" stroke=\"{stroke}\" stroke-width=\"{width:.1}\" stroke-linecap=\"round\" transform=\"rotate(-90 {cx:.1} {cy:.1})\" stroke-dashoffset=\"{dashoffset:.1}\"").ok();
    if let Some(opacity) = opacity {
        write!(svg, " opacity=\"{opacity}\"").ok();
    }
    if let Some(dasharray) = dasharray {
        write!(svg, " stroke-dasharray=\"{dasharray}\"").ok();
    }
    svg.push_str("/>");
}

#[allow(clippy::too_many_arguments)]
fn line(
    svg: &mut String,
    x1: f64,
    y1: f64,
    x2: f64,
    y2: f64,
    stroke: &str,
    width: f64,
    opacity: Option<&str>,
) {
    write!(svg, "<line x1=\"{x1:.1}\" y1=\"{y1:.1}\" x2=\"{x2:.1}\" y2=\"{y2:.1}\" stroke=\"{stroke}\" stroke-width=\"{width:.1}\"").ok();
    if let Some(opacity) = opacity {
        write!(svg, " opacity=\"{opacity}\"").ok();
    }
    svg.push_str("/>");
}

fn path(svg: &mut String, data: &str, stroke: &str, width: f64, fill: &str) {
    write!(svg, "<path d=\"{data}\" stroke=\"{stroke}\" stroke-width=\"{width:.1}\" fill=\"{fill}\" stroke-linecap=\"round\" stroke-linejoin=\"round\"/>").ok();
}

fn xml(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

// Space Grotesk (SIL OFL 1.1, see assets/fonts/OFL.txt) is embedded so exported
// PNGs render identical display type regardless of the user's installed fonts.
static DISPLAY_FONT_MEDIUM: &[u8] = include_bytes!("../assets/fonts/SpaceGrotesk-Medium.ttf");
static DISPLAY_FONT_BOLD: &[u8] = include_bytes!("../assets/fonts/SpaceGrotesk-Bold.ttf");

fn render_png(svg: &str, path: &Path) -> AppResult<()> {
    let bytes = render_png_bytes(svg)?;
    std::fs::write(path, bytes)?;
    Ok(())
}

fn render_png_bytes(svg: &str) -> AppResult<Vec<u8>> {
    let mut options = resvg::usvg::Options::default();
    let fontdb = options.fontdb_mut();
    fontdb.load_font_data(DISPLAY_FONT_MEDIUM.to_vec());
    fontdb.load_font_data(DISPLAY_FONT_BOLD.to_vec());
    fontdb.load_system_fonts();
    let tree = resvg::usvg::Tree::from_data(svg.as_bytes(), &options)
        .map_err(|error| AppError::Render(error.to_string()))?;
    let size = tree.size().to_int_size();
    let mut pixmap = resvg::tiny_skia::Pixmap::new(size.width(), size.height())
        .ok_or_else(|| AppError::Render("image dimensions are invalid".into()))?;
    resvg::render(
        &tree,
        resvg::tiny_skia::Transform::default(),
        &mut pixmap.as_mut(),
    );
    pixmap
        .encode_png()
        .map_err(|error| AppError::Render(error.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_required_aspects_have_exact_dimensions() {
        assert_eq!(dimensions("1:1").expect("square"), (2400, 2400));
        assert_eq!(dimensions("2:3").expect("classic portrait"), (1920, 2880));
        assert_eq!(dimensions("3:2").expect("classic landscape"), (2880, 1920));
        assert_eq!(dimensions("3:4").expect("portrait"), (2160, 2880));
        assert_eq!(dimensions("4:3").expect("landscape"), (2400, 1800));
        assert_eq!(dimensions("4:5").expect("portrait"), (2160, 2700));
        assert_eq!(dimensions("16:9").expect("wide"), (2560, 1440));
        assert_eq!(dimensions("9:16").expect("story"), (1440, 2560));
    }

    #[test]
    fn long_text_is_bounded_and_escaped() {
        let lines = wrap_text(
            "这是一个非常长的标题 with <private> content that keeps going",
            320.0,
            32.0,
            2,
        );
        assert!(lines.len() <= 2);
        assert_eq!(xml("A&B<C>"), "A&amp;B&lt;C&gt;");
    }

    #[test]
    fn vcti_visual_uses_readable_dimension_labels() {
        assert_eq!(
            vcti_dimension_name("zh-CN", "requirementClarity"),
            "目标清晰"
        );
        assert_eq!(
            vcti_dimension_name("en-US", "parallelOrchestration"),
            "Parallel"
        );
        assert_ne!(vcti_dimension_name("zh-CN", "contextReuse"), "18");
    }

    #[test]
    fn vcti_share_copy_uses_the_same_agent_foreman_hook() {
        assert_eq!(vcti_type_name("zh-CN", "BOSS"), "Agent 包工头");
        assert_eq!(
            vcti_type_tagline("zh-CN", "BOSS"),
            "别人把 Agent 当助手，你已经给它们排班、派活、验收。"
        );
    }

    #[test]
    fn vcti_share_range_caption_follows_the_selected_range() {
        assert_eq!(vcti_range_caption("zh-CN", "30d"), "月 · 本机推断");
        assert_eq!(
            vcti_range_caption("en-US", "year"),
            "YEAR · LOCAL INFERENCE"
        );
        assert_ne!(vcti_range_caption("zh-CN", "30d"), "最近 90 天 · 本机推断");
    }

    #[test]
    fn vcti_card_keeps_the_original_character_art() {
        let mut svg = String::new();

        render_vcti_avatar(
            &mut svg,
            20.0,
            30.0,
            400.0,
            "SPEC",
            "start",
            "#245CA6",
            Palette::for_theme("light"),
        );

        assert!(svg.contains("data:image/webp;base64,"));
        assert!(!svg.contains("data-vcti-visual-version"));
        let png = render_png_bytes(&format!(
            "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"500\" height=\"500\">{svg}</svg>"
        ))
        .expect("character art PNG");
        assert!(png.starts_with(b"\x89PNG\r\n\x1a\n"));
    }

    #[test]
    fn vcti_identity_evidence_keeps_zero_distinct_from_unrecorded() {
        let zero = crate::models::VctiOptionalMetric {
            value: Some(0.0),
            available: true,
        };
        let missing = crate::models::VctiOptionalMetric {
            value: None,
            available: false,
        };

        assert_eq!(vcti_optional_count("zh-CN", &zero), "0 次");
        assert_eq!(vcti_optional_count("zh-CN", &missing), "未记录");
        assert_eq!(vcti_optional_count("en-US", &zero), "0");
        assert_eq!(vcti_optional_count("en-US", &missing), "Not recorded");
    }

    #[test]
    fn unavailable_vcti_dimension_does_not_render_a_value_bar() {
        let dimension = crate::models::VctiScore {
            id: "requirementClarity".into(),
            value: 92.0,
            coverage: 0.0,
        };
        let mut svg = String::new();
        let palette = Palette::for_theme("light");
        rect(
            &mut svg,
            10.0,
            10.0,
            20.0,
            100.0,
            10.0,
            palette.hairline,
            None,
        );
        render_vcti_dimension_value(
            &mut svg, 10.0, 10.0, 20.0, 100.0, "#EE7532", palette, &dimension,
        );

        assert_eq!(svg.matches("<rect").count(), 1);
        assert!(svg.contains("<line"));
        assert!(!svg.contains("#EE7532"));
    }

    #[test]
    fn savitzky_golay_preserves_expected_usage_shapes() {
        let constant = savitzky_golay_smooth(&[5.0, 5.0, 5.0, 5.0, 5.0, 5.0, 5.0]);
        assert!(constant.iter().all(|value| (*value - 5.0).abs() < 1e-10));
        let quadratic = savitzky_golay_smooth(&[0.0, 1.0, 4.0, 9.0, 16.0, 25.0, 36.0]);
        assert!((quadratic[3] - 9.0).abs() < 1e-10);
        let short = savitzky_golay_smooth(&[1.0, 2.0, 3.0, 4.0]);
        assert_eq!(short, vec![1.0, 2.0, 3.0, 4.0]);
        let impulse = savitzky_golay_smooth(&[0.0, 0.0, 0.0, 21.0, 0.0, 0.0, 0.0]);
        assert!(impulse.iter().all(|value| *value >= 0.0));
    }

    #[test]
    fn usage_trend_has_four_descending_axis_ticks() {
        assert_eq!(
            trend_tick_values(3_000_000.0),
            [3_000_000, 2_000_000, 1_000_000, 0]
        );
    }

    #[test]
    fn catchphrase_share_uses_a_champion_frame_and_honest_empty_state() {
        let directory = tempfile::tempdir().expect("tempdir");
        let database = Database::open(directory.path().join("vibemeter.sqlite")).expect("database");
        let request = ShareRenderRequest {
            template_id: "catchphrases".into(),
            locale: "zh-CN".into(),
            aspect_ratio: "1:1".into(),
            theme: "dark".into(),
            range: "30d".into(),
            session_id: None,
            compare_ids: Vec::new(),
            title: String::new(),
            summary: String::new(),
            project_name: String::new(),
            metrics: Vec::new(),
            show_brand: true,
            show_model: false,
            show_cost: false,
            show_project: false,
            show_behavior_evidence: false,
            privacy_reviewed: true,
        };
        let result = preview(&database, request).expect("preview");
        assert!(result.svg.contains("它最离不开的一句"));
        assert!(result.svg.contains("至少需要两个会话重复同一句短语"));
        assert!(result.svg.contains("github.com/RangeKing/vibemeter"));
        let png = render_png_bytes(&result.svg).expect("png");
        assert!(png.starts_with(b"\x89PNG\r\n\x1a\n"));
    }

    #[test]
    fn catchphrase_share_promotes_one_model_attributed_verbal_tic() {
        use crate::models::{AgentKind, ParseState, PhraseAggregate};

        let directory = tempfile::tempdir().expect("tempdir");
        let database = Database::open(directory.path().join("vibemeter.sqlite")).expect("database");
        let date = chrono::Local::now()
            .date_naive()
            .format("%Y-%m-%d")
            .to_string();
        for (index, occurrences) in [3_u64, 1_u64].into_iter().enumerate() {
            let id = format!("phrase-share-{index}");
            let mut state = ParseState::new(AgentKind::Codex, id.clone());
            state.started_at = Some(format!("{date}T02:00:00Z"));
            state.ended_at = Some(format!("{date}T02:10:00Z"));
            state.current_model = Some("gpt-5.4".into());
            state.phrase_counts.insert(
                "agent".into(),
                PhraseAggregate {
                    date: date.clone(),
                    role: "agent".into(),
                    phrase: "我会先检查".into(),
                    occurrences,
                },
            );
            database
                .persist_parse_state(&id, 1, 1, 1, &state)
                .expect("phrase session");
        }
        let request = ShareRenderRequest {
            template_id: "catchphrases".into(),
            locale: "zh-CN".into(),
            aspect_ratio: "1:1".into(),
            theme: "light".into(),
            range: "30d".into(),
            session_id: None,
            compare_ids: Vec::new(),
            title: String::new(),
            summary: String::new(),
            project_name: String::new(),
            metrics: Vec::new(),
            show_brand: true,
            show_model: false,
            show_cost: false,
            show_project: false,
            show_behavior_evidence: false,
            privacy_reviewed: true,
        };
        let result = preview(&database, request).expect("preview");
        assert!(result.svg.contains("我会先检查"));
        assert!(result.svg.contains("gpt-5.4 · Codex"));
        assert!(result.svg.contains("4×"));
        assert!(result.svg.contains("横跨 2 个会话"));
        assert!(result.svg.contains("不说这句，看来就没法开工。"));
        assert!(!result.svg.contains("我的口头禅"));
    }

    // Renders every template against a real DB copy so cards can be inspected visually.
    // Run with: cargo test render_sample_cards -- --ignored --nocapture
    #[test]
    #[ignore]
    fn render_sample_cards() {
        let db_path = std::path::PathBuf::from("/tmp/av/av.sqlite");
        let database = Database::open(db_path).expect("open db");
        let out = std::path::PathBuf::from("/tmp/av/cards");
        std::fs::create_dir_all(&out).expect("mkdir");
        let templates = [
            "usage-overview",
            "developer-wrapped",
            "agent-comparison",
            "session-recap",
            "vcti-card",
            "catchphrases",
        ];
        let combos = [
            ("zh-CN", "1:1", "light"),
            ("en-US", "2:3", "dark"),
            ("zh-CN", "3:2", "light"),
            ("zh-CN", "3:4", "dark"),
            ("en-US", "4:3", "dark"),
            ("zh-CN", "9:16", "light"),
            ("zh-CN", "16:9", "dark"),
            ("en-US", "4:5", "light"),
        ];
        for template in templates {
            for (locale, aspect, theme) in combos {
                let request = ShareRenderRequest {
                    template_id: template.to_string(),
                    locale: locale.to_string(),
                    aspect_ratio: aspect.to_string(),
                    theme: theme.to_string(),
                    range: if template == "vcti-card" {
                        "90d"
                    } else {
                        "30d"
                    }
                    .to_string(),
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
                match preview(&database, request) {
                    Ok(preview) => {
                        let stem = format!("{template}_{}_{}", aspect.replace(':', "-"), theme);
                        let name = format!("{stem}_{locale}");
                        std::fs::write(out.join(format!("{name}.svg")), &preview.svg).ok();
                        match render_png_bytes(&preview.svg) {
                            Ok(bytes) => {
                                std::fs::write(out.join(format!("{name}.png")), bytes).ok();
                            }
                            Err(error) => eprintln!("PNG FAIL {name}: {error}"),
                        }
                    }
                    Err(error) => eprintln!("PREVIEW FAIL {template} {locale} {aspect}: {error}"),
                }
            }
        }
        eprintln!("cards written to {}", out.display());
    }
}
