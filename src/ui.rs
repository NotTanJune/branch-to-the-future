use std::time::Duration as StdDuration;

use image::{imageops::FilterType, ImageReader};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style, Stylize},
    text::{Line, Span},
    widgets::{Block, Borders, Gauge, List, ListItem, Paragraph, Wrap},
    Frame,
};
use tachyonfx::Duration as FxDuration;

use crate::{
    app::App,
    domain::{AnimationStage, ImpactAnalysis, ImpactFile, ImplementationFuture, Screen},
};

pub fn render(frame: &mut Frame<'_>, app: &mut App, frame_delta: StdDuration) {
    let fx_area = fx_area_for_screen(frame.area(), app.screen, app.animation_stage);
    match app.screen {
        Screen::Input => render_input(frame, app),
        Screen::RepoScan => render_scan(frame, app),
        Screen::ImpactExplorer => render_explorer(frame, app),
        Screen::FileDetail => render_file_detail(frame, app),
        Screen::FuturesCompare => render_futures(frame, app),
        Screen::ArtifactGeneration => render_artifact(frame, app),
        Screen::ExportSummary => render_export(frame, app),
        Screen::Error => render_error(frame, app),
        Screen::Help => render_help(frame, app),
    }
    let millis = frame_delta.as_millis().clamp(16, 120) as u32;
    app.effects
        .process_effects(FxDuration::from_millis(millis), frame.buffer_mut(), fx_area);
}

fn render_input(frame: &mut Frame<'_>, app: &App) {
    let area = centered(frame.area(), 82, 14);
    let cursor = if app.cursor_visible { "█" } else { " " };
    let input = format!("{}{}", app.change_request, cursor);
    let text = vec![
        Line::from("Change impact simulator").dark_gray(),
        Line::from(""),
        Line::from("Describe proposed change:"),
        Line::from(input).yellow(),
        Line::from(""),
        Line::from("Enter analyze | q quit | ? help").dark_gray(),
        Line::from(format!("Repo: {}", app.repo_path.display())).dark_gray(),
    ];
    frame.render_widget(panel("Branch Futures", text), area);
}

fn render_scan(frame: &mut Frame<'_>, app: &App) {
    let area = centered(frame.area(), 104, 26);
    let sections = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Min(18),
        ])
        .split(area);
    let spinner = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"][app.spinner_frame % 10];
    let gauge = Gauge::default()
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("Branch Futures"),
        )
        .gauge_style(Style::default().fg(Color::DarkGray).bg(Color::Black))
        .ratio(((app.spinner_frame % 20) as f64 + 1.0) / 20.0)
        .label(Span::styled(
            format!("{spinner} {}", app.status),
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        ));
    frame.render_widget(gauge, sections[0]);

    let scan_summary = app
        .repo_model
        .as_ref()
        .map(|repo| {
            format!(
                "Repo materialized: {} files | {} routes | {} tests | {} frameworks",
                repo.files.len(),
                repo.routes.len(),
                repo.tests.len(),
                repo.frameworks.len()
            )
        })
        .unwrap_or_else(|| "Indexing tree, routes, tests, schemas, workers".to_string());
    frame.render_widget(
        Paragraph::new(scan_summary)
            .block(Block::default().borders(Borders::ALL).title("Scan Trace"))
            .wrap(Wrap { trim: true }),
        sections[1],
    );

    let preview = if app.stream_preview.is_empty() {
        "Waiting for OpenAI stream events".to_string()
    } else {
        app.stream_preview.clone()
    };
    frame.render_widget(
        Paragraph::new(preview)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("OpenAI Stream"),
            )
            .wrap(Wrap { trim: false }),
        sections[2],
    );
}

fn render_explorer(frame: &mut Frame<'_>, app: &App) {
    let outer = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(5),
            Constraint::Length(3),
        ])
        .split(frame.area());
    frame.render_widget(
        Paragraph::new(format!("Change: {}", app.change_request))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("Branch Futures: Impact Explorer"),
            )
            .wrap(Wrap { trim: true }),
        outer[0],
    );
    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(64), Constraint::Percentage(36)])
        .split(outer[1]);
    render_repo_tree(frame, app, columns[0]);
    render_impact_path(frame, app, columns[1]);
    frame.render_widget(
        Paragraph::new(app.status.as_str())
            .block(Block::default().borders(Borders::ALL).title("Status")),
        outer[2],
    );
}

fn render_repo_tree(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let impacted: Vec<String> = app
        .impact_analysis
        .as_ref()
        .map(|analysis| {
            analysis
                .impact_path
                .iter()
                .take(app.trace_revealed)
                .map(|file| file.path.clone())
                .collect()
        })
        .unwrap_or_default();
    let items = app
        .repo_model
        .as_ref()
        .map(|repo| {
            repo.files
                .iter()
                .take(36)
                .map(|file| {
                    let marker = if impacted.contains(&file.path) {
                        ">"
                    } else {
                        " "
                    };
                    let style = if impacted.contains(&file.path) {
                        Style::default()
                            .fg(Color::Yellow)
                            .add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(Color::Gray)
                    };
                    ListItem::new(Line::from(vec![
                        Span::raw(format!("{marker} ")),
                        Span::styled(file.path.clone(), style),
                    ]))
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_else(|| vec![ListItem::new("No repo model yet")]);
    frame.render_widget(
        List::new(items).block(Block::default().borders(Borders::ALL).title("Repo Tree")),
        area,
    );
}

fn render_impact_path(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let items = app
        .impact_analysis
        .as_ref()
        .map(|analysis| {
            analysis
                .impact_path
                .iter()
                .take(app.trace_revealed)
                .enumerate()
                .map(|(index, file)| {
                    let selected = index == app.selected_file_index;
                    let line_style = if selected {
                        Style::default()
                            .fg(Color::Black)
                            .bg(Color::Cyan)
                            .add_modifier(Modifier::BOLD)
                    } else {
                        risk_color(file)
                    };
                    ListItem::new(Line::from(vec![
                        Span::styled(format!("{:>2}.  ", index + 1), line_style),
                        Span::styled(file.path.clone(), line_style),
                        Span::styled(
                            format!("  {:>2}/10", score_out_of_10(file.impact_score)),
                            line_style,
                        ),
                    ]))
                    .style(line_style)
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_else(|| vec![ListItem::new("Waiting for OpenAI")]);
    frame.render_widget(
        List::new(items).block(
            Block::default()
                .borders(Borders::ALL)
                .title("Impact Path | Enter inspect | Tab futures | e export"),
        ),
        area,
    );
}

fn render_file_detail(frame: &mut Frame<'_>, app: &App) {
    let file = app
        .impact_analysis
        .as_ref()
        .and_then(|analysis| analysis.impact_path.get(app.selected_file_index));
    let Some(file) = file else {
        render_explorer(frame, app);
        return;
    };
    let lines = vec![
        Line::from(format!("File: {}", file.path)).cyan().bold(),
        Line::from(format!(
            "Impact: {}/10     Confidence: {}%     Risk: {}",
            score_out_of_10(file.impact_score),
            file.confidence,
            file.risk
        )),
        Line::from(""),
        Line::from("Why affected").yellow(),
        Line::from(format!("- {}", file.reason)),
        Line::from(""),
        Line::from("Suggested changes").yellow(),
        Line::from(format!("- {}", file.change_needed)),
        Line::from(""),
        Line::from("Esc back | j/k next file | q quit").dark_gray(),
    ];
    frame.render_widget(panel("File Impact", lines), centered(frame.area(), 88, 18));
}

fn render_futures(frame: &mut Frame<'_>, app: &App) {
    let futures = app
        .impact_analysis
        .as_ref()
        .map(|analysis| analysis.futures.as_slice())
        .unwrap_or(&[]);
    let outer = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(9),
            Constraint::Min(6),
            Constraint::Length(3),
        ])
        .split(frame.area());
    let items = futures
        .iter()
        .enumerate()
        .map(|(index, future)| {
            let selected = index == app.selected_future_index;
            let style = if selected {
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Cyan)
                    .add_modifier(Modifier::BOLD)
            } else {
                risk_color_for_level(&future.risk)
            };
            ListItem::new(Line::from(vec![
                Span::styled(format!("{:>2}.  ", index + 1), style),
                Span::styled(future.name.clone(), style),
                Span::styled(
                    format!(
                        "    Complexity: {}    Risk: {}",
                        future.complexity, future.risk
                    ),
                    style,
                ),
            ]))
            .style(style)
        })
        .collect::<Vec<_>>();
    frame.render_widget(
        List::new(items).block(
            Block::default()
                .borders(Borders::ALL)
                .title("Branch Futures"),
        ),
        outer[0],
    );
    let selected = futures.get(app.selected_future_index);
    render_future_detail(frame, selected, outer[1]);
    frame.render_widget(
        Paragraph::new("j/k select | e export | Esc back | q quit")
            .block(Block::default().borders(Borders::ALL).title("Keys")),
        outer[2],
    );
}

fn render_future_detail(frame: &mut Frame<'_>, future: Option<&ImplementationFuture>, area: Rect) {
    let Some(future) = future else {
        frame.render_widget(Paragraph::new("No futures returned"), area);
        return;
    };
    let mut lines = vec![
        Line::from(format!("Selected: {}", future.name))
            .cyan()
            .bold(),
        Line::from(future.description.as_str()),
        Line::from(""),
        Line::from("Affected files").yellow(),
    ];
    lines.extend(
        future
            .affected_files
            .iter()
            .map(|file| Line::from(format!("- {file}"))),
    );
    lines.push(Line::from(""));
    lines.push(Line::from("Patch plan").yellow());
    lines.extend(
        future
            .patch_plan
            .iter()
            .map(|step| Line::from(format!("- {step}"))),
    );
    frame.render_widget(
        Paragraph::new(lines)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("Selected Future"),
            )
            .wrap(Wrap { trim: true }),
        area,
    );
}

fn render_export(frame: &mut Frame<'_>, app: &App) {
    let path = app
        .export_path
        .as_ref()
        .map(|path| path.display().to_string())
        .unwrap_or_else(|| "No export path".to_string());
    frame.render_widget(
        panel(
            "Export Summary",
            vec![
                Line::from("Markdown report written").green().bold(),
                Line::from(path),
                Line::from(""),
                Line::from("Enter or Esc back | q quit").dark_gray(),
            ],
        ),
        centered(frame.area(), 88, 10),
    );
}

fn render_error(frame: &mut Frame<'_>, app: &App) {
    frame.render_widget(
        panel(
            "Error",
            vec![
                Line::from(app.error.as_deref().unwrap_or("Unknown error")).red(),
                Line::from(""),
                Line::from("r retry input | Esc input | q quit").dark_gray(),
            ],
        ),
        centered(frame.area(), 88, 12),
    );
}

fn render_help(frame: &mut Frame<'_>, _app: &App) {
    frame.render_widget(
        panel(
            "Help",
            vec![
                Line::from("q quit"),
                Line::from("Esc back"),
                Line::from("Tab switch explorer/futures"),
                Line::from("Enter inspect or confirm"),
                Line::from("r replay trace"),
                Line::from("p patch skeleton status"),
                Line::from("t test plan status"),
                Line::from("e export Markdown"),
                Line::from("g image blueprint status"),
            ],
        ),
        centered(frame.area(), 72, 16),
    );
}

fn panel<'a>(title: &'a str, lines: Vec<Line<'a>>) -> Paragraph<'a> {
    Paragraph::new(lines)
        .block(Block::default().borders(Borders::ALL).title(title))
        .wrap(Wrap { trim: true })
}

fn centered(area: Rect, width: u16, height: u16) -> Rect {
    let horizontal = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Min(0),
            Constraint::Length(width.min(area.width)),
            Constraint::Min(0),
        ])
        .split(area);
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(0),
            Constraint::Length(height.min(area.height)),
            Constraint::Min(0),
        ])
        .split(horizontal[1]);
    vertical[1]
}

fn fx_area_for_screen(area: Rect, screen: Screen, stage: AnimationStage) -> Rect {
    match screen {
        Screen::Input => centered(area, 82, 14),
        Screen::RepoScan => scan_fx_area(area, stage),
        Screen::ImpactExplorer => {
            let outer = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(3),
                    Constraint::Min(5),
                    Constraint::Length(3),
                ])
                .split(area);
            let columns = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Percentage(64), Constraint::Percentage(36)])
                .split(outer[1]);
            columns[1]
        }
        Screen::FuturesCompare => centered(
            area,
            area.width.saturating_sub(8),
            area.height.saturating_sub(6),
        ),
        Screen::ArtifactGeneration => artifact_fx_area(area),
        Screen::FileDetail => centered(area, 88, 18),
        Screen::ExportSummary => centered(area, 88, 10),
        Screen::Error => centered(area, 88, 12),
        Screen::Help => centered(area, 72, 16),
    }
}

fn scan_fx_area(area: Rect, stage: AnimationStage) -> Rect {
    let area = centered(area, 104, 26);
    let sections = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Min(18),
        ])
        .split(area);
    match stage {
        AnimationStage::ScanningSweep => sections[1],
        AnimationStage::StreamShimmer => sections[2],
        _ => area,
    }
}

fn artifact_fx_area(area: Rect) -> Rect {
    let area = centered(
        area,
        area.width.saturating_sub(8),
        area.height.saturating_sub(4),
    );
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(14),
            Constraint::Length(8),
            Constraint::Length(3),
        ])
        .split(area);
    chunks[0]
}

fn risk_color(file: &ImpactFile) -> Style {
    risk_color_for_level(&file.risk)
}

fn risk_color_for_level(risk: &crate::domain::RiskLevel) -> Style {
    match risk {
        crate::domain::RiskLevel::Low => Style::default().fg(Color::Green),
        crate::domain::RiskLevel::Medium => Style::default().fg(Color::Yellow),
        crate::domain::RiskLevel::High => Style::default().fg(Color::Red),
    }
}

fn score_out_of_10(score: u8) -> u8 {
    ((score as f32 / 10.0).round() as u8).clamp(0, 10)
}

fn render_artifact(frame: &mut Frame<'_>, app: &App) {
    let area = centered(
        frame.area(),
        frame.area().width.saturating_sub(8),
        frame.area().height.saturating_sub(4),
    );
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(14),
            Constraint::Length(8),
            Constraint::Length(3),
        ])
        .split(area);

    let title = app
        .image_path
        .as_ref()
        .map(|path| format!("Architecture Diagram | {}", path.display()))
        .unwrap_or_else(|| "Architecture Diagram".to_string());

    render_diagram_preview(frame, app, chunks[0], title);
    render_diagram_overlays(frame, app, chunks[0], chunks[1]);

    frame.render_widget(
        Paragraph::new("Esc back | e export Markdown | q quit")
            .block(Block::default().borders(Borders::ALL).title("Keys")),
        chunks[2],
    );
}

fn render_diagram_preview(frame: &mut Frame<'_>, app: &App, area: Rect, title: String) {
    let lines = match &app.image_path {
        Some(path) if path.exists() => image_preview_lines(path, area),
        Some(path) => vec![
            Line::from("Generating architecture blueprint...")
                .cyan()
                .bold(),
            Line::from(""),
            Line::from("Readable labels are rendered by Branch Futures in the terminal.")
                .dark_gray(),
            Line::from(path.display().to_string()).dark_gray(),
        ],
        None => vec![
            Line::from("Press g after analysis to generate architecture blueprint")
                .cyan()
                .bold(),
            Line::from("Readable labels are rendered by Branch Futures in the terminal.")
                .dark_gray(),
        ],
    };
    frame.render_widget(
        Paragraph::new(lines)
            .block(Block::default().borders(Borders::ALL).title(title))
            .wrap(Wrap { trim: false }),
        area,
    );
}

fn render_diagram_overlays(
    frame: &mut Frame<'_>,
    app: &App,
    diagram_area: Rect,
    bottom_area: Rect,
) {
    let overlay_area = Rect {
        x: diagram_area.x.saturating_add(1),
        y: diagram_area.y.saturating_add(1),
        width: diagram_area.width.saturating_sub(2),
        height: diagram_area.height.saturating_sub(2),
    };
    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(34),
            Constraint::Percentage(33),
            Constraint::Percentage(33),
        ])
        .split(overlay_area);
    frame.render_widget(
        panel(
            "Impacted Files",
            diagram_impact_lines(app.impact_analysis.as_ref()),
        ),
        columns[0],
    );
    frame.render_widget(
        panel(
            "Selected Trace",
            diagram_trace_lines(app.impact_analysis.as_ref(), app.selected_file_index),
        ),
        columns[1],
    );
    frame.render_widget(
        panel(
            "Branch Futures",
            diagram_future_lines(app.impact_analysis.as_ref(), app.selected_future_index),
        ),
        columns[2],
    );

    let saved_path = app
        .image_path
        .as_ref()
        .map(|path| path.display().to_string())
        .unwrap_or_else(|| "No PNG generated yet".to_string());
    let recommendation = app
        .impact_analysis
        .as_ref()
        .map(|analysis| analysis.recommended_future.as_str())
        .unwrap_or("Waiting for impact analysis");
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(format!("Recommended path: {recommendation}"))
                .green()
                .bold(),
            Line::from(format!("PNG artifact: {saved_path}")).dark_gray(),
        ])
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("Recommendation"),
        )
        .wrap(Wrap { trim: true }),
        bottom_area,
    );
}

fn diagram_impact_lines(analysis: Option<&ImpactAnalysis>) -> Vec<Line<'static>> {
    let Some(analysis) = analysis else {
        return vec![Line::from("Waiting for impact analysis").dark_gray()];
    };
    if analysis.impact_path.is_empty() {
        return vec![Line::from("No impacted files returned").dark_gray()];
    }
    analysis
        .impact_path
        .iter()
        .take(6)
        .map(|file| {
            Line::from(vec![
                Span::styled(
                    file.path.clone(),
                    risk_color(file).add_modifier(Modifier::BOLD),
                ),
                Span::raw(format!(
                    "  {}/10  {}",
                    score_out_of_10(file.impact_score),
                    file.risk
                )),
            ])
        })
        .collect()
}

fn diagram_trace_lines(
    analysis: Option<&ImpactAnalysis>,
    selected_file_index: usize,
) -> Vec<Line<'static>> {
    let Some(analysis) = analysis else {
        return vec![Line::from("Waiting for trace").dark_gray()];
    };
    if analysis.impact_path.is_empty() {
        return vec![Line::from("No trace path returned").dark_gray()];
    }
    analysis
        .impact_path
        .iter()
        .take(8)
        .enumerate()
        .map(|(index, file)| {
            let marker = if index == selected_file_index {
                ">"
            } else {
                "-"
            };
            let style = if index == selected_file_index {
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Cyan)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Gray)
            };
            Line::from(vec![
                Span::styled(format!("{marker} {:>2}. ", index + 1), style),
                Span::styled(file.path.clone(), style),
            ])
        })
        .collect()
}

fn diagram_future_lines(
    analysis: Option<&ImpactAnalysis>,
    selected_future_index: usize,
) -> Vec<Line<'static>> {
    let Some(analysis) = analysis else {
        return vec![Line::from("Waiting for futures").dark_gray()];
    };
    if analysis.futures.is_empty() {
        return vec![Line::from("No futures returned").dark_gray()];
    }
    analysis
        .futures
        .iter()
        .take(4)
        .enumerate()
        .map(|(index, future)| {
            let selected = index == selected_future_index;
            let style = if selected {
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Cyan)
                    .add_modifier(Modifier::BOLD)
            } else {
                risk_color_for_level(&future.risk)
            };
            Line::from(vec![
                Span::styled(format!("{:>2}. ", index + 1), style),
                Span::styled(future.name.clone(), style),
                Span::styled(format!("  {}  {}", future.complexity, future.risk), style),
            ])
        })
        .collect()
}

fn image_preview_lines(path: &std::path::Path, area: Rect) -> Vec<Line<'static>> {
    let inner_width = area.width.saturating_sub(2).max(1) as u32;
    let inner_height = area.height.saturating_sub(2).max(1) as u32;
    let pixel_height = inner_height.saturating_mul(2).max(2);
    let image = match ImageReader::open(path)
        .and_then(|reader| reader.with_guessed_format())
        .and_then(|reader| reader.decode().map_err(std::io::Error::other))
    {
        Ok(image) => image.to_rgb8(),
        Err(error) => {
            return vec![
                Line::from("Could not render image preview").red(),
                Line::from(error.to_string()).dark_gray(),
            ]
        }
    };
    let resized = image::imageops::resize(&image, inner_width, pixel_height, FilterType::Triangle);
    let mut lines = Vec::new();
    for y in (0..resized.height()).step_by(2) {
        let mut spans = Vec::new();
        for x in 0..resized.width() {
            let top = resized.get_pixel(x, y);
            let bottom = resized.get_pixel(x, (y + 1).min(resized.height() - 1));
            spans.push(Span::styled(
                "▀",
                Style::default()
                    .fg(Color::Rgb(top[0], top[1], top[2]))
                    .bg(Color::Rgb(bottom[0], bottom[1], bottom[2])),
            ));
        }
        lines.push(Line::from(spans));
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{Rgb, RgbImage};
    use ratatui::{backend::TestBackend, Terminal};

    use crate::{
        cli::{Cli, ReasoningEffort},
        domain::{Complexity, ImpactFile, ImplementationFuture, RiskLevel},
    };

    fn app() -> App {
        let dir = tempfile::tempdir().unwrap();
        let cli = Cli::parse_from(["brf", dir.path().to_str().unwrap()]);
        let mut app = App::from_cli(cli, "test-key".to_string());
        app.reasoning_effort = ReasoningEffort::Low;
        app.screen = Screen::ArtifactGeneration;
        app.impact_analysis = Some(analysis());
        app
    }

    fn analysis() -> ImpactAnalysis {
        ImpactAnalysis {
            summary: "Async upload".to_string(),
            impact_path: vec![ImpactFile {
                path: "app/api/upload/route.ts".to_string(),
                reason: "entrypoint".to_string(),
                impact_score: 82,
                confidence: 90,
                risk: RiskLevel::High,
                change_needed: "enqueue".to_string(),
            }],
            risk_summary: vec!["PII".to_string()],
            tests_to_add: vec!["job id".to_string()],
            futures: vec![ImplementationFuture {
                name: "Queue Worker".to_string(),
                description: "Async parsing".to_string(),
                complexity: Complexity::Medium,
                risk: RiskLevel::Low,
                affected_files: vec!["workers/parser.ts".to_string()],
                benefits: vec![],
                drawbacks: vec![],
                patch_plan: vec![],
                test_plan: vec![],
            }],
            recommended_future: "Queue Worker".to_string(),
        }
    }

    #[test]
    fn diagram_overlay_labels_keep_scores_out_of_10() {
        let analysis = analysis();
        let lines = diagram_impact_lines(Some(&analysis));
        let rendered = lines
            .iter()
            .flat_map(|line| line.spans.iter())
            .map(|span| span.content.as_ref())
            .collect::<String>();

        assert!(rendered.contains("app/api/upload/route.ts"));
        assert!(rendered.contains("8/10"));
    }

    #[test]
    fn diagram_screen_renders_without_existing_png() {
        let mut app = app();
        app.image_path = Some(std::path::PathBuf::from("/tmp/missing-branch-futures.png"));
        let backend = TestBackend::new(120, 40);
        let mut terminal = Terminal::new(backend).unwrap();

        terminal
            .draw(|frame| render(frame, &mut app, StdDuration::from_millis(16)))
            .unwrap();
    }

    #[test]
    fn diagram_screen_renders_with_existing_png() {
        let mut app = app();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("branch-futures-architecture.png");
        let image = RgbImage::from_pixel(4, 4, Rgb([10, 20, 30]));
        image.save(&path).unwrap();
        app.image_path = Some(path);
        let backend = TestBackend::new(120, 40);
        let mut terminal = Terminal::new(backend).unwrap();

        terminal
            .draw(|frame| render(frame, &mut app, StdDuration::from_millis(16)))
            .unwrap();
    }
}
