use std::{collections::BTreeSet, time::Duration as StdDuration};

use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style, Stylize},
    text::{Line, Span},
    widgets::{Block, Borders, Gauge, List, ListItem, Paragraph, Wrap},
    Frame,
};
use tachyonfx::Duration as FxDuration;

use crate::{
    app::{impact_score_out_of_10, App},
    domain::{
        AnimationStage, ArchitectureStage, ArchitectureZoom, FileKind, ImpactAnalysis, ImpactFile,
        ImplementationFuture, RepoFile, RepoModel, RiskSignal, RouteInfo, Screen,
    },
};

const COMPACT_BOX_WIDTH: usize = 28;

pub fn render(frame: &mut Frame<'_>, app: &mut App, frame_delta: StdDuration) {
    let fx_area = fx_area_for_screen(frame.area(), app.screen, app.animation_stage);
    match app.screen {
        Screen::Input => render_input(frame, app),
        Screen::RepoScan => render_scan(frame, app),
        Screen::ImpactExplorer => render_explorer(frame, app),
        Screen::FileDetail => render_file_detail(frame, app),
        Screen::FuturesCompare => render_futures(frame, app),
        Screen::RepoTree => render_repo_tree_screen(frame, app),
        Screen::ArchitectureScaffold => render_architecture(frame, app),
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
        Line::from(key_hints(Screen::Input)).dark_gray(),
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
            Constraint::Min(15),
            Constraint::Length(3),
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
    frame.render_widget(
        Paragraph::new(key_hints(Screen::RepoScan))
            .block(Block::default().borders(Borders::ALL).title("Keys")),
        sections[3],
    );
}

fn render_explorer(frame: &mut Frame<'_>, app: &App) {
    let outer = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(5),
            Constraint::Length(3),
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
    frame.render_widget(
        Paragraph::new(key_hints(Screen::ImpactExplorer))
            .block(Block::default().borders(Borders::ALL).title("Keys")),
        outer[3],
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
        List::new(items).block(
            Block::default()
                .borders(Borders::ALL)
                .title("Repo Tree | T repo tree"),
        ),
        area,
    );
}

fn render_repo_tree_screen(frame: &mut Frame<'_>, app: &App) {
    let outer = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(8), Constraint::Length(3)])
        .split(frame.area());
    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(44), Constraint::Percentage(56)])
        .split(outer[0]);

    let items = app
        .repo_model
        .as_ref()
        .map(|repo| {
            if repo.files.is_empty() {
                return vec![ListItem::new("No files in repo")];
            }
            repo.files
                .iter()
                .enumerate()
                .map(|(index, file)| {
                    let selected = index == app.selected_repo_file_index;
                    let style = if selected {
                        Style::default()
                            .fg(Color::Black)
                            .bg(Color::Cyan)
                            .add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(Color::Gray)
                    };
                    ListItem::new(Line::from(vec![
                        Span::styled(format!("{:>2}.  ", index + 1), style),
                        Span::styled(file.path.clone(), style),
                    ]))
                    .style(style)
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_else(|| vec![ListItem::new("No repo model yet")]);
    frame.render_widget(
        List::new(items).block(
            Block::default()
                .borders(Borders::ALL)
                .title("Repository Files"),
        ),
        columns[0],
    );

    let selected_file = app
        .repo_model
        .as_ref()
        .and_then(|repo| repo.files.get(app.selected_repo_file_index));
    frame.render_widget(
        Paragraph::new(repo_file_detail_lines(selected_file))
            .block(Block::default().borders(Borders::ALL).title("File Details"))
            .wrap(Wrap { trim: false }),
        columns[1],
    );

    frame.render_widget(
        Paragraph::new(key_hints(Screen::RepoTree))
            .block(Block::default().borders(Borders::ALL).title("Keys")),
        outer[1],
    );
}

fn repo_file_detail_lines(file: Option<&RepoFile>) -> Vec<Line<'static>> {
    let Some(file) = file else {
        return vec![Line::from("No file selected")];
    };
    let mut lines = vec![
        Line::from(format!("File: {}", file.path)).cyan().bold(),
        Line::from(format!(
            "Kind: {}     Size: {} bytes",
            file_kind_label(&file.kind),
            file.size
        )),
        Line::from(""),
        Line::from("Symbols").yellow(),
    ];
    lines.extend(detail_items(&file.symbols));
    lines.push(Line::from(""));
    lines.push(Line::from("Imports").yellow());
    lines.extend(detail_items(&file.imports));
    lines.push(Line::from(""));
    lines.push(Line::from("Snippets").yellow());
    lines.extend(detail_items(&file.snippets));
    lines
}

fn detail_items(items: &[String]) -> Vec<Line<'static>> {
    if items.is_empty() {
        return vec![Line::from("- none").dark_gray()];
    }
    items
        .iter()
        .take(6)
        .map(|item| Line::from(format!("- {item}")))
        .collect()
}

fn file_kind_label(kind: &FileKind) -> &'static str {
    match kind {
        FileKind::JavaScript => "javascript",
        FileKind::TypeScript => "typescript",
        FileKind::Python => "python",
        FileKind::Rust => "rust",
        FileKind::Config => "config",
        FileKind::Route => "route",
        FileKind::Test => "test",
        FileKind::Schema => "schema",
        FileKind::Worker => "worker",
        FileKind::Service => "service",
        FileKind::Controller => "controller",
        FileKind::UiComponent => "ui component",
        FileKind::Unknown => "unknown",
    }
}

fn render_impact_path(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let items = app
        .impact_analysis
        .as_ref()
        .map(|analysis| {
            app.visible_impact_indices()
                .into_iter()
                .enumerate()
                .filter_map(|(index, file_index)| {
                    let file = analysis.impact_path.get(file_index)?;
                    let selected = index == app.selected_file_index;
                    let line_style = if selected {
                        Style::default()
                            .fg(Color::Black)
                            .bg(Color::Cyan)
                            .add_modifier(Modifier::BOLD)
                    } else {
                        risk_color(file)
                    };
                    Some(
                        ListItem::new(Line::from(vec![
                            Span::styled(format!("{:>2}.  ", index + 1), line_style),
                            Span::styled(file.path.clone(), line_style),
                            Span::styled(
                                format!("  {:>2}/10", impact_score_out_of_10(file.impact_score)),
                                line_style,
                            ),
                        ]))
                        .style(line_style),
                    )
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_else(|| vec![ListItem::new("Waiting for OpenAI")]);
    frame.render_widget(
        List::new(items).block(Block::default().borders(Borders::ALL).title(format!(
            "Impact Path | sort: {} | s change",
            app.impact_sort
        ))),
        area,
    );
}

fn render_file_detail(frame: &mut Frame<'_>, app: &App) {
    let file = app.selected_impact_file();
    let Some(file) = file else {
        render_explorer(frame, app);
        return;
    };
    let lines = vec![
        Line::from(format!("File: {}", file.path)).cyan().bold(),
        Line::from(format!(
            "Impact: {}/10     Confidence: {}%     Risk: {}",
            impact_score_out_of_10(file.impact_score),
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
        Line::from(key_hints(Screen::FileDetail)).dark_gray(),
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
        Paragraph::new(key_hints(Screen::FuturesCompare))
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
                Line::from(key_hints(Screen::ExportSummary)).dark_gray(),
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
                Line::from(key_hints(Screen::Error)).dark_gray(),
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
                Line::from("s change impact sort"),
                Line::from("T repo tree"),
                Line::from("e export Markdown"),
                Line::from("g architecture"),
                Line::from(""),
                Line::from(key_hints(Screen::Help)).dark_gray(),
            ],
        ),
        centered(frame.area(), 72, 18),
    );
}

fn key_hints(screen: Screen) -> &'static str {
    match screen {
        Screen::Input => "Enter scan | ? help | q quit",
        Screen::RepoScan => "? help | q quit",
        Screen::ImpactExplorer => {
            "j/k select | Enter inspect | s sort | Tab futures | T repo tree | g architecture | e export | ? help | q quit"
        }
        Screen::FileDetail => {
            "j/k next file | s sort | Esc explorer | T repo tree | g architecture | ? help | q quit"
        }
        Screen::FuturesCompare => {
            "j/k select | Tab/Esc explorer | T repo tree | g architecture | e export | ? help | q quit"
        }
        Screen::RepoTree => "j/k select | Esc back | g architecture | e export | ? help | q quit",
        Screen::ArchitectureScaffold => {
            "h/j/k/l pan | arrows pan | -/+ zoom | 0 reset | Esc back | T repo tree | e export | ? help | q quit"
        }
        Screen::ExportSummary => "Enter/Esc back | q quit",
        Screen::Error => "r retry | Enter/Esc input | q quit",
        Screen::Help => "Enter/Esc back | q quit",
    }
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
    if is_screen_transition(stage) {
        return area;
    }

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
        Screen::RepoTree => area,
        Screen::ArchitectureScaffold => architecture_fx_area(area),
        Screen::FileDetail => centered(area, 88, 18),
        Screen::ExportSummary => centered(area, 88, 10),
        Screen::Error => centered(area, 88, 12),
        Screen::Help => centered(area, 72, 18),
    }
}

fn is_screen_transition(stage: AnimationStage) -> bool {
    matches!(
        stage,
        AnimationStage::RepoMaterialize
            | AnimationStage::ImpactToFutures
            | AnimationStage::FuturesToImpact
            | AnimationStage::DiagramReveal
    )
}

fn scan_fx_area(area: Rect, stage: AnimationStage) -> Rect {
    let area = centered(area, 104, 26);
    let sections = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Min(15),
            Constraint::Length(3),
        ])
        .split(area);
    match stage {
        AnimationStage::ScanningSweep => sections[1],
        AnimationStage::StreamShimmer => sections[2],
        _ => area,
    }
}

fn architecture_fx_area(area: Rect) -> Rect {
    let area = centered(
        area,
        area.width.saturating_sub(2),
        area.height.saturating_sub(4),
    );
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(22),
            Constraint::Length(4),
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

fn render_architecture(frame: &mut Frame<'_>, app: &App) {
    let area = centered(
        frame.area(),
        frame.area().width.saturating_sub(2),
        frame.area().height.saturating_sub(4),
    );
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(22),
            Constraint::Length(4),
            Constraint::Length(3),
        ])
        .split(area);

    render_architecture_scaffold(frame, app, chunks[0]);
    render_architecture_summary(frame, app, chunks[1]);

    frame.render_widget(
        Paragraph::new(key_hints(Screen::ArchitectureScaffold))
            .block(Block::default().borders(Borders::ALL).title("Keys")),
        chunks[2],
    );
}

fn render_architecture_scaffold(frame: &mut Frame<'_>, app: &App, area: Rect) {
    frame.render_widget(
        Paragraph::new(architecture_diagram_lines(
            app.repo_model.as_ref(),
            app.impact_analysis.as_ref(),
            app.selected_future_index,
            app.architecture_zoom,
        ))
        .block(Block::default().borders(Borders::ALL).title(format!(
            "ASCII System Map | zoom {} | row {} col {}",
            app.architecture_zoom, app.architecture_scroll_y, app.architecture_scroll_x
        )))
        .scroll((app.architecture_scroll_y, app.architecture_scroll_x)),
        area,
    );
}

struct DiagramBox {
    title: String,
    items: Vec<String>,
}

fn architecture_diagram_lines(
    repo: Option<&RepoModel>,
    analysis: Option<&ImpactAnalysis>,
    selected_future_index: usize,
    zoom: ArchitectureZoom,
) -> Vec<Line<'static>> {
    architecture_diagram_strings_with_zoom(repo, analysis, selected_future_index, zoom)
        .into_iter()
        .map(|line| Line::from(Span::styled(line.clone(), architecture_line_style(&line))))
        .collect()
}

#[cfg(test)]
fn architecture_diagram_strings(
    repo: Option<&RepoModel>,
    analysis: Option<&ImpactAnalysis>,
    selected_future_index: usize,
) -> Vec<String> {
    architecture_diagram_strings_with_zoom(
        repo,
        analysis,
        selected_future_index,
        ArchitectureZoom::Normal,
    )
}

fn architecture_diagram_strings_with_zoom(
    repo: Option<&RepoModel>,
    analysis: Option<&ImpactAnalysis>,
    selected_future_index: usize,
    zoom: ArchitectureZoom,
) -> Vec<String> {
    let repo_name = repo
        .map(|repo| repo.repo_name.as_str())
        .unwrap_or("waiting for repo scan");
    let frameworks = repo
        .and_then(|repo| (!repo.frameworks.is_empty()).then(|| repo.frameworks.join(", ")))
        .unwrap_or_else(|| "frameworks pending".to_string());
    let file_count = repo.map(|repo| repo.files.len()).unwrap_or_default();
    let route_count = repo.map(|repo| repo.routes.len()).unwrap_or_default();
    let test_count = repo.map(|repo| repo.tests.len()).unwrap_or_default();
    let first_test = repo
        .and_then(|repo| repo.tests.first())
        .map(|test| format!(" | test: {test}"))
        .unwrap_or_default();
    let future_name = selected_future(analysis, selected_future_index)
        .map(|future| future.name.as_str())
        .unwrap_or("future pending");

    let mut lines = vec![
        format!(
            "SYSTEM MAP: {repo_name} | {frameworks} | {file_count} files, {route_count} routes, {test_count} tests | future: {future_name}{first_test}"
        ),
        String::new(),
    ];
    lines.push("CURRENT FLOW | CURRENT ARCHITECTURE".to_string());
    lines.extend(boxed_flow_rows(&current_flow_boxes(repo, analysis), zoom));
    lines.push(String::new());
    lines.push("PROPOSED FLOW | PROPOSED ARCHITECTURE".to_string());
    lines.extend(boxed_flow_rows(
        &proposed_flow_boxes(repo, analysis, selected_future_index),
        zoom,
    ));
    lines.push(String::new());
    lines.push("CHANGE SET".to_string());
    lines.extend(change_set_lines(repo, analysis, selected_future_index));
    lines.push(String::new());
    lines.push("RISK SIGNALS".to_string());
    lines.extend(risk_signal_lines(repo, analysis).into_iter().take(1));
    lines
}

fn architecture_line_style(line: &str) -> Style {
    if line.starts_with("SYSTEM MAP") {
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD)
    } else if line.starts_with("FRAMEWORKS") || line.starts_with("SCAN") {
        Style::default().fg(Color::Blue)
    } else if line.starts_with("CURRENT") {
        Style::default()
            .fg(Color::Blue)
            .add_modifier(Modifier::BOLD)
    } else if line.starts_with("PROPOSED") {
        Style::default()
            .fg(Color::Green)
            .add_modifier(Modifier::BOLD)
    } else if line.starts_with('+') || line.contains("+--") {
        Style::default().fg(Color::DarkGray)
    } else if line.contains("-->") || line.trim() == "v" || line.contains(" v ") {
        Style::default().fg(Color::Cyan)
    } else if line.starts_with("CHANGE SET") {
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD)
    } else if line.contains("+ ADD") {
        Style::default()
            .fg(Color::Green)
            .add_modifier(Modifier::BOLD)
    } else if line.contains("CHANGE") {
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD)
    } else if line.starts_with("RISK SIGNALS") {
        Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::Gray)
    }
}

fn current_flow_boxes(
    repo: Option<&RepoModel>,
    analysis: Option<&ImpactAnalysis>,
) -> Vec<DiagramBox> {
    if let Some(boxes) =
        analysis.and_then(|analysis| analysis_stage_boxes(&analysis.current_architecture))
    {
        return boxes;
    }

    vec![
        DiagramBox {
            title: "UI SURFACE".to_string(),
            items: ui_labels(repo),
        },
        DiagramBox {
            title: "ENTRYPOINTS".to_string(),
            items: entrypoint_labels(repo, analysis),
        },
        DiagramBox {
            title: "APP CORE".to_string(),
            items: core_labels(repo, analysis),
        },
        DiagramBox {
            title: "DATA / CONFIG".to_string(),
            items: data_labels(repo),
        },
    ]
}

fn proposed_flow_boxes(
    repo: Option<&RepoModel>,
    analysis: Option<&ImpactAnalysis>,
    selected_future_index: usize,
) -> Vec<DiagramBox> {
    if let Some(boxes) = selected_future(analysis, selected_future_index)
        .and_then(|future| analysis_stage_boxes(&future.architecture))
    {
        return boxes;
    }

    vec![
        DiagramBox {
            title: "UI SURFACE".to_string(),
            items: ui_labels(repo),
        },
        DiagramBox {
            title: "CHANGE ENTRY".to_string(),
            items: proposed_entrypoint_labels(repo, analysis),
        },
        DiagramBox {
            title: "+ ADD QUEUE".to_string(),
            items: proposed_queue_labels(repo, analysis, selected_future_index),
        },
        DiagramBox {
            title: "CHANGE WORKER".to_string(),
            items: proposed_worker_labels(repo, analysis, selected_future_index),
        },
        DiagramBox {
            title: "DATA / CONFIG".to_string(),
            items: data_labels(repo),
        },
    ]
}

fn boxed_flow_rows(boxes: &[DiagramBox], zoom: ArchitectureZoom) -> Vec<String> {
    if boxes.is_empty() {
        return Vec::new();
    }
    let rendered = boxes
        .iter()
        .map(|box_spec| rendered_box(box_spec, zoom))
        .collect::<Vec<_>>();
    let widths = rendered
        .iter()
        .map(|box_spec| box_spec.width)
        .collect::<Vec<_>>();
    let borders = widths
        .iter()
        .map(|width| box_border(*width))
        .collect::<Vec<_>>();
    let max_titles = rendered
        .iter()
        .map(|box_spec| box_spec.title.len())
        .max()
        .unwrap_or(0);
    let max_items = rendered
        .iter()
        .map(|box_spec| box_spec.items.len())
        .max()
        .unwrap_or(0);
    let mut rows = vec![borders.join("     ")];
    for title_index in 0..max_titles {
        rows.push(
            rendered
                .iter()
                .zip(widths.iter())
                .map(|(box_spec, width)| {
                    box_line(
                        box_spec
                            .title
                            .get(title_index)
                            .map(String::as_str)
                            .unwrap_or(""),
                        *width,
                    )
                })
                .collect::<Vec<_>>()
                .join(if title_index == 0 { " --> " } else { "     " }),
        );
    }
    rows.push(borders.join("     "));
    for item_index in 0..max_items {
        rows.push(
            rendered
                .iter()
                .zip(widths.iter())
                .map(|(box_spec, width)| {
                    box_line(
                        box_spec
                            .items
                            .get(item_index)
                            .map(String::as_str)
                            .unwrap_or(""),
                        *width,
                    )
                })
                .collect::<Vec<_>>()
                .join("     "),
        );
    }
    rows.push(borders.join("     "));
    rows
}

struct RenderedBox {
    title: Vec<String>,
    items: Vec<String>,
    width: usize,
}

fn rendered_box(box_spec: &DiagramBox, zoom: ArchitectureZoom) -> RenderedBox {
    let width = match zoom {
        ArchitectureZoom::Normal => box_width(box_spec),
        ArchitectureZoom::Compact => box_width(box_spec).min(COMPACT_BOX_WIDTH),
    };
    RenderedBox {
        title: wrap_box_value(&box_spec.title, width, zoom),
        items: box_spec
            .items
            .iter()
            .flat_map(|item| wrap_box_value(item, width, zoom))
            .collect(),
        width,
    }
}

fn box_width(box_spec: &DiagramBox) -> usize {
    box_spec
        .items
        .iter()
        .map(|item| item.chars().count())
        .chain(std::iter::once(box_spec.title.chars().count()))
        .max()
        .unwrap_or(0)
        .max(18)
}

fn wrap_box_value(value: &str, width: usize, zoom: ArchitectureZoom) -> Vec<String> {
    match zoom {
        ArchitectureZoom::Normal => vec![value.to_string()],
        ArchitectureZoom::Compact => wrap_words(value, width),
    }
}

fn wrap_words(value: &str, width: usize) -> Vec<String> {
    let mut lines = Vec::new();
    let mut current = String::new();
    for word in value.split_whitespace() {
        if word.chars().count() > width {
            if !current.is_empty() {
                lines.push(current);
                current = String::new();
            }
            lines.extend(split_long_word(word, width));
            continue;
        }
        let next_len = if current.is_empty() {
            word.chars().count()
        } else {
            current.chars().count() + 1 + word.chars().count()
        };
        if next_len > width && !current.is_empty() {
            lines.push(current);
            current = word.to_string();
        } else {
            if !current.is_empty() {
                current.push(' ');
            }
            current.push_str(word);
        }
    }
    if !current.is_empty() {
        lines.push(current);
    }
    if lines.is_empty() {
        lines.push(String::new());
    }
    lines
}

fn split_long_word(word: &str, width: usize) -> Vec<String> {
    let mut chunks = Vec::new();
    let mut current = String::new();
    for ch in word.chars() {
        if current.chars().count() == width {
            chunks.push(current);
            current = String::new();
        }
        current.push(ch);
    }
    if !current.is_empty() {
        chunks.push(current);
    }
    chunks
}

fn box_border(width: usize) -> String {
    format!("+{}+", "-".repeat(width + 2))
}

fn box_line(value: &str, width: usize) -> String {
    format!("| {:<width$} |", value, width = width)
}

fn analysis_stage_boxes(stages: &[ArchitectureStage]) -> Option<Vec<DiagramBox>> {
    let boxes = stages
        .iter()
        .filter_map(|stage| {
            let label = stage.label.trim();
            if label.is_empty() {
                return None;
            }
            Some(DiagramBox {
                title: label.to_string(),
                items: stage_items(stage),
            })
        })
        .collect::<Vec<_>>();
    (!boxes.is_empty()).then_some(boxes)
}

fn stage_items(stage: &ArchitectureStage) -> Vec<String> {
    let mut items = Vec::new();
    for responsibility in &stage.responsibilities {
        let value = responsibility.trim();
        if !value.is_empty() {
            push_unique(&mut items, value.to_string());
        }
    }
    for file in &stage.files {
        let value = file.trim();
        if !value.is_empty() {
            push_unique(&mut items, value.to_string());
        }
    }
    with_placeholder_all(items, "analysis stage pending")
}

fn entrypoint_labels(repo: Option<&RepoModel>, analysis: Option<&ImpactAnalysis>) -> Vec<String> {
    let mut labels = repo
        .into_iter()
        .flat_map(|repo| repo.routes.iter())
        .take(3)
        .map(route_label)
        .collect::<Vec<_>>();
    if labels.is_empty() {
        labels = repo_file_labels(repo, &[FileKind::Route], 3);
    }
    if labels.is_empty() {
        labels = impact_file_labels(analysis, 2);
    }
    with_placeholder(labels, "no entrypoints found")
}

fn core_labels(repo: Option<&RepoModel>, analysis: Option<&ImpactAnalysis>) -> Vec<String> {
    let mut labels = repo_file_labels(repo, &[FileKind::Service, FileKind::Controller], 3);
    if labels.is_empty() {
        labels = repo_file_labels(
            repo,
            &[
                FileKind::TypeScript,
                FileKind::JavaScript,
                FileKind::Python,
                FileKind::Rust,
            ],
            3,
        );
    }
    if labels.is_empty() {
        labels = impact_file_labels(analysis, 2);
    }
    with_placeholder(labels, "core pending")
}

fn data_labels(repo: Option<&RepoModel>) -> Vec<String> {
    let mut labels = repo_file_labels(repo, &[FileKind::Schema], 2);
    if let Some(repo) = repo {
        labels.extend(repo.config_files.iter().take(2).cloned());
    }
    with_placeholder(labels, "no data/config files")
}

fn ui_labels(repo: Option<&RepoModel>) -> Vec<String> {
    with_placeholder(
        repo_file_labels(repo, &[FileKind::UiComponent], 3),
        "no UI surface found",
    )
}

fn current_worker_labels(repo: Option<&RepoModel>) -> Vec<String> {
    with_placeholder(
        repo_file_labels(repo, &[FileKind::Worker], 3),
        "no workers found",
    )
}

fn proposed_entrypoint_labels(
    repo: Option<&RepoModel>,
    analysis: Option<&ImpactAnalysis>,
) -> Vec<String> {
    let changed = impacted_paths(analysis);
    let mut labels = repo
        .into_iter()
        .flat_map(|repo| repo.routes.iter())
        .filter(|route| changed.contains(&route.path))
        .take(3)
        .map(|route| format!("CHANGE {}", route.path))
        .collect::<Vec<_>>();
    if labels.is_empty() {
        labels = impact_file_labels(analysis, 2)
            .into_iter()
            .map(|path| format!("CHANGE {path}"))
            .collect();
    }
    with_placeholder(labels, "entry unchanged")
}

fn proposed_queue_labels(
    repo: Option<&RepoModel>,
    analysis: Option<&ImpactAnalysis>,
    selected_future_index: usize,
) -> Vec<String> {
    let repo_paths = repo_paths(repo);
    let mut labels = selected_future(analysis, selected_future_index)
        .map(|future| {
            future
                .affected_files
                .iter()
                .filter(|path| !repo_paths.contains(*path))
                .map(|path| format!("+ ADD {path}"))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    if labels.is_empty() {
        labels = selected_future(analysis, selected_future_index)
            .map(|future| vec![format!("+ ADD {}", future.name)])
            .unwrap_or_default();
    }
    with_placeholder_all(labels, "no new queue layer")
}

fn proposed_worker_labels(
    repo: Option<&RepoModel>,
    analysis: Option<&ImpactAnalysis>,
    selected_future_index: usize,
) -> Vec<String> {
    let repo_paths = repo_paths(repo);
    let mut labels = selected_future(analysis, selected_future_index)
        .map(|future| {
            future
                .affected_files
                .iter()
                .filter(|path| repo_paths.contains(*path))
                .map(|path| format!("CHANGE {path}"))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    if labels.is_empty() {
        labels = current_worker_labels(repo);
    }
    with_placeholder_all(labels, "worker unchanged")
}

fn repo_file_labels(repo: Option<&RepoModel>, kinds: &[FileKind], limit: usize) -> Vec<String> {
    repo.into_iter()
        .flat_map(|repo| repo.files.iter())
        .filter(|file| kinds.iter().any(|kind| kind == &file.kind))
        .take(limit)
        .map(file_label)
        .collect()
}

fn impact_file_labels(analysis: Option<&ImpactAnalysis>, limit: usize) -> Vec<String> {
    analysis
        .into_iter()
        .flat_map(|analysis| analysis.impact_path.iter())
        .take(limit)
        .map(|file| file.path.clone())
        .collect()
}

fn route_label(route: &RouteInfo) -> String {
    format!("{} {}", route.method, route.route)
}

fn file_label(file: &RepoFile) -> String {
    file.path.clone()
}

fn with_placeholder(mut labels: Vec<String>, placeholder: &str) -> Vec<String> {
    if labels.is_empty() {
        labels.push(placeholder.to_string());
    }
    labels.truncate(3);
    labels
}

fn with_placeholder_all(mut labels: Vec<String>, placeholder: &str) -> Vec<String> {
    if labels.is_empty() {
        labels.push(placeholder.to_string());
    }
    labels
}

fn selected_future(
    analysis: Option<&ImpactAnalysis>,
    selected_future_index: usize,
) -> Option<&ImplementationFuture> {
    analysis
        .and_then(|analysis| analysis.futures.get(selected_future_index))
        .or_else(|| analysis.and_then(|analysis| analysis.futures.first()))
}

fn change_set_lines(
    repo: Option<&RepoModel>,
    analysis: Option<&ImpactAnalysis>,
    selected_future_index: usize,
) -> Vec<String> {
    let repo_paths = repo_paths(repo);
    let mut labels = Vec::new();
    if let Some(analysis) = analysis {
        for file in analysis.impact_path.iter().take(3) {
            push_unique(
                &mut labels,
                format!(
                    "CHANGE {}  {}/10 {}",
                    file.path,
                    impact_score_out_of_10(file.impact_score),
                    file.risk
                ),
            );
        }
    }
    if let Some(future) = selected_future(analysis, selected_future_index) {
        for path in &future.affected_files {
            let marker = if repo_paths.contains(path) {
                "CHANGE"
            } else {
                "+ ADD"
            };
            push_unique(&mut labels, format!("{marker} {path}"));
        }
    }
    with_placeholder_all(labels, "no file changes selected")
}

fn repo_paths(repo: Option<&RepoModel>) -> BTreeSet<String> {
    repo.into_iter()
        .flat_map(|repo| repo.files.iter())
        .map(|file| file.path.clone())
        .collect()
}

fn impacted_paths(analysis: Option<&ImpactAnalysis>) -> BTreeSet<String> {
    analysis
        .into_iter()
        .flat_map(|analysis| analysis.impact_path.iter())
        .map(|file| file.path.clone())
        .collect()
}

fn push_unique(labels: &mut Vec<String>, label: String) {
    if !labels.iter().any(|existing| existing == &label) {
        labels.push(label);
    }
}

fn risk_signal_lines(repo: Option<&RepoModel>, analysis: Option<&ImpactAnalysis>) -> Vec<String> {
    let mut lines = repo
        .into_iter()
        .flat_map(|repo| repo.risk_signals.iter())
        .take(3)
        .map(risk_signal_label)
        .collect::<Vec<_>>();
    if lines.is_empty() {
        lines = analysis
            .map(|analysis| {
                analysis
                    .risk_summary
                    .iter()
                    .take(3)
                    .map(|risk| format!("  {risk}"))
                    .collect()
            })
            .unwrap_or_default();
    }
    with_placeholder(lines, "  no risk signals detected")
}

fn risk_signal_label(signal: &RiskSignal) -> String {
    format!("  {}: {}", signal.path, signal.signal)
}

fn render_architecture_summary(frame: &mut Frame<'_>, app: &App, area: Rect) {
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
            Line::from("Scaffold source: terminal layout only, no generated image").dark_gray(),
        ])
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("Recommendation"),
        )
        .wrap(Wrap { trim: true }),
        area,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{backend::TestBackend, Terminal};

    use crate::{
        cli::{Cli, ReasoningEffort},
        domain::{
            ArchitectureStage, Complexity, FileKind, ImpactFile, ImplementationFuture, RepoFile,
            RepoModel, RiskLevel, RiskSignal, RouteInfo,
        },
    };

    fn app() -> App {
        let dir = tempfile::tempdir().unwrap();
        let cli = Cli::parse_from(["brf", dir.path().to_str().unwrap()]);
        let mut app = App::from_cli(cli, "test-key".to_string());
        app.reasoning_effort = ReasoningEffort::Low;
        app.screen = Screen::ArchitectureScaffold;
        app.impact_analysis = Some(analysis());
        app.repo_model = Some(repo_model());
        app
    }

    fn repo_model() -> RepoModel {
        RepoModel {
            repo_name: "resume-interview".to_string(),
            root_path: "/tmp/resume-interview".to_string(),
            frameworks: vec!["Next.js".to_string(), "TypeScript".to_string()],
            files: vec![
                RepoFile {
                    path: "app/api/upload/route.ts".to_string(),
                    kind: FileKind::Route,
                    size: 1200,
                    symbols: vec!["POST".to_string()],
                    imports: vec!["services/parser".to_string()],
                    snippets: vec![],
                },
                RepoFile {
                    path: "services/parser.ts".to_string(),
                    kind: FileKind::Service,
                    size: 800,
                    symbols: vec!["parseResume".to_string()],
                    imports: vec!["db/resumes".to_string()],
                    snippets: vec![],
                },
                RepoFile {
                    path: "workers/parser.ts".to_string(),
                    kind: FileKind::Worker,
                    size: 650,
                    symbols: vec!["parseJob".to_string()],
                    imports: vec![],
                    snippets: vec![],
                },
                RepoFile {
                    path: "db/schema.sql".to_string(),
                    kind: FileKind::Schema,
                    size: 400,
                    symbols: vec![],
                    imports: vec![],
                    snippets: vec![],
                },
                RepoFile {
                    path: "components/Uploader.tsx".to_string(),
                    kind: FileKind::UiComponent,
                    size: 1000,
                    symbols: vec!["Uploader".to_string()],
                    imports: vec!["app/api/upload/route".to_string()],
                    snippets: vec![],
                },
                RepoFile {
                    path: "tests/upload.test.ts".to_string(),
                    kind: FileKind::Test,
                    size: 700,
                    symbols: vec!["uploads resume".to_string()],
                    imports: vec![],
                    snippets: vec![],
                },
            ],
            routes: vec![RouteInfo {
                path: "app/api/upload/route.ts".to_string(),
                method: "POST".to_string(),
                route: "/api/upload".to_string(),
            }],
            tests: vec!["tests/upload.test.ts".to_string()],
            config_files: vec!["next.config.ts".to_string()],
            risk_signals: vec![RiskSignal {
                path: "app/api/upload/route.ts".to_string(),
                signal: "file upload without visible validation".to_string(),
            }],
        }
    }

    fn analysis() -> ImpactAnalysis {
        ImpactAnalysis {
            summary: "Async upload".to_string(),
            current_architecture: vec![],
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
                architecture: vec![],
                affected_files: vec![
                    "workers/parser.ts".to_string(),
                    "queue/resume-jobs.ts".to_string(),
                ],
                benefits: vec![],
                drawbacks: vec![],
                patch_plan: vec!["Add queue producer before parsing".to_string()],
                test_plan: vec!["job id".to_string()],
            }],
            recommended_future: "Queue Worker".to_string(),
        }
    }

    #[test]
    fn diagram_overlay_labels_keep_scores_out_of_10() {
        let repo = repo_model();
        let analysis = analysis();
        let rendered = architecture_diagram_strings(Some(&repo), Some(&analysis), 0).join("\n");

        assert!(rendered.contains("app/api/upload/route.ts"));
        assert!(rendered.contains("8/10"));
    }

    #[test]
    fn architecture_screen_renders_tui_scaffold() {
        let mut app = app();
        let backend = TestBackend::new(120, 40);
        let mut terminal = Terminal::new(backend).unwrap();

        terminal
            .draw(|frame| render(frame, &mut app, StdDuration::from_millis(16)))
            .unwrap();
    }

    #[test]
    fn architecture_screen_renders_scroll_and_zoom_state() {
        let mut app = app();
        app.architecture_zoom = ArchitectureZoom::Compact;
        app.architecture_scroll_x = 8;
        app.architecture_scroll_y = 2;
        let backend = TestBackend::new(120, 40);
        let mut terminal = Terminal::new(backend).unwrap();

        terminal
            .draw(|frame| render(frame, &mut app, StdDuration::from_millis(16)))
            .unwrap();

        let rendered = terminal.backend().to_string();
        assert!(rendered.contains("zoom compact"));
        assert!(rendered.contains("row 2 col 8"));
    }

    #[test]
    fn architecture_diagram_uses_repo_scan_layers() {
        let app = app();
        let diagram = architecture_diagram_strings(
            app.repo_model.as_ref(),
            app.impact_analysis.as_ref(),
            app.selected_future_index,
        )
        .join("\n");

        assert!(diagram.contains("+"));
        assert!(diagram.contains("| UI SURFACE"));
        assert!(diagram.contains("| ENTRYPOINTS"));
        assert!(diagram.contains("-->"));
        assert!(diagram.contains("Next.js, TypeScript"));
        assert!(diagram.contains("POST /api/upload"));
        assert!(diagram.contains("services/parser.ts"));
        assert!(diagram.contains("workers/parser.ts"));
        assert!(diagram.contains("db/schema.sql"));
        assert!(diagram.contains("tests/upload.test.ts"));
        assert!(diagram.contains("app/api/upload/route.ts"));
        assert!(diagram.contains("Queue Worker"));
        assert!(diagram.contains("queue/resume-jobs.ts"));
        assert!(diagram.contains("file upload without visible validation"));
    }

    #[test]
    fn architecture_diagram_prefers_analysis_generated_stage_labels() {
        let mut app = app();
        if let Some(analysis) = &mut app.impact_analysis {
            analysis.current_architecture = vec![
                ArchitectureStage {
                    label: "CLI INGESTION".to_string(),
                    responsibilities: vec!["parse GitHub URL or local path".to_string()],
                    files: vec!["src/cli.rs".to_string(), "src/repo_source.rs".to_string()],
                },
                ArchitectureStage {
                    label: "RUST ANALYZER".to_string(),
                    responsibilities: vec!["index symbols and imports".to_string()],
                    files: vec!["src/repo.rs".to_string()],
                },
            ];
            analysis.futures[0].architecture = vec![
                ArchitectureStage {
                    label: "TOKEN-AWARE CLONE".to_string(),
                    responsibilities: vec!["clone private GitHub repos".to_string()],
                    files: vec!["src/repo_source.rs".to_string()],
                },
                ArchitectureStage {
                    label: "TEMP REPO ANALYSIS".to_string(),
                    responsibilities: vec!["read architecture from clone".to_string()],
                    files: vec!["src/app.rs".to_string(), "src/ai.rs".to_string()],
                },
            ];
        }

        let diagram = architecture_diagram_strings(
            app.repo_model.as_ref(),
            app.impact_analysis.as_ref(),
            app.selected_future_index,
        )
        .join("\n");

        assert!(diagram.contains("| CLI INGESTION"));
        assert!(diagram.contains("| RUST ANALYZER"));
        assert!(diagram.contains("| TOKEN-AWARE CLONE"));
        assert!(diagram.contains("| TEMP REPO ANALYSIS"));
        assert!(diagram.contains("src/repo_source.rs"));
        assert!(!diagram.contains("| UI SURFACE"));
        assert!(!diagram.contains("| APP CORE"));
    }

    #[test]
    fn architecture_compact_zoom_reduces_map_width_without_truncation() {
        let mut app = app();
        if let Some(analysis) = &mut app.impact_analysis {
            analysis.current_architecture = vec![
                ArchitectureStage {
                    label: "CLI orchestration and very long fallback search stage".to_string(),
                    responsibilities: vec![
                        "Parse scan/find/watch commands and build scan options".to_string(),
                        "Trigger indexing and reporting selected paths".to_string(),
                    ],
                    files: vec!["src/main.rs".to_string(), "src/watch.rs".to_string()],
                },
                ArchitectureStage {
                    label: "SQLite search index".to_string(),
                    responsibilities: vec![
                        "Manage local index databases and secondary indexes".to_string()
                    ],
                    files: vec!["src/db.rs".to_string(), "src/query.rs".to_string()],
                },
            ];
        }

        let normal = architecture_diagram_strings_with_zoom(
            app.repo_model.as_ref(),
            app.impact_analysis.as_ref(),
            app.selected_future_index,
            ArchitectureZoom::Normal,
        );
        let compact = architecture_diagram_strings_with_zoom(
            app.repo_model.as_ref(),
            app.impact_analysis.as_ref(),
            app.selected_future_index,
            ArchitectureZoom::Compact,
        );
        let normal_width = normal.iter().map(|line| line.len()).max().unwrap_or(0);
        let compact_width = compact.iter().map(|line| line.len()).max().unwrap_or(0);
        let compact_rendered = compact.join("\n");

        assert!(compact_width < normal_width);
        assert!(compact_rendered.contains("fallback search stage"));
        assert!(compact_rendered.contains("src/query.rs"));
        assert!(!compact_rendered.contains('~'));
    }

    #[test]
    fn architecture_diagram_compares_current_and_proposed_flows() {
        let app = app();
        let diagram = architecture_diagram_strings(
            app.repo_model.as_ref(),
            app.impact_analysis.as_ref(),
            app.selected_future_index,
        )
        .join("\n");

        assert!(diagram.contains("CURRENT FLOW"));
        assert!(diagram.contains("PROPOSED FLOW"));
        assert!(diagram.contains("CHANGE app/api/upload/route.ts"));
        assert!(diagram.contains("CHANGE workers/parser.ts"));
        assert!(diagram.contains("+ ADD queue/resume-jobs.ts"));
        assert!(diagram.contains("CURRENT ARCHITECTURE"));
        assert!(diagram.contains("PROPOSED ARCHITECTURE"));
    }

    #[test]
    fn architecture_diagram_keeps_long_change_paths_untruncated() {
        let mut app = app();
        let long_existing = "services/recommendations/pipeline/deeply-nested-score-writer.ts";
        let long_new = "workers/recommendations/async-score-rebuild-worker.ts";
        if let Some(repo) = &mut app.repo_model {
            repo.files.push(RepoFile {
                path: long_existing.to_string(),
                kind: FileKind::Service,
                size: 900,
                symbols: vec!["writeRecommendationScores".to_string()],
                imports: vec![],
                snippets: vec![],
            });
        }
        if let Some(analysis) = &mut app.impact_analysis {
            analysis.impact_path.push(ImpactFile {
                path: long_existing.to_string(),
                reason: "score persistence changes".to_string(),
                impact_score: 9,
                confidence: 88,
                risk: RiskLevel::High,
                change_needed: "route writes through worker".to_string(),
            });
            analysis.futures[0].affected_files =
                vec![long_existing.to_string(), long_new.to_string()];
        }

        let diagram = architecture_diagram_strings(
            app.repo_model.as_ref(),
            app.impact_analysis.as_ref(),
            app.selected_future_index,
        )
        .join("\n");

        assert!(diagram.contains(long_existing));
        assert!(diagram.contains(long_new));
        assert!(diagram.contains("| CHANGE WORKER"));
        assert!(diagram.contains("| + ADD QUEUE"));
        assert!(!diagram.contains('~'));
    }

    #[test]
    fn architecture_diagram_lists_all_change_set_paths() {
        let mut app = app();
        let affected = [
            "app/api/recommendations/route.ts",
            "services/recommendations/parser.ts",
            "services/recommendations/scorer.ts",
            "workers/recommendations/rebuild.ts",
            "db/recommendation-score.sql",
        ];
        if let Some(analysis) = &mut app.impact_analysis {
            analysis.futures[0].affected_files =
                affected.iter().map(|path| path.to_string()).collect();
        }

        let diagram = architecture_diagram_strings(
            app.repo_model.as_ref(),
            app.impact_analysis.as_ref(),
            app.selected_future_index,
        )
        .join("\n");

        for path in affected {
            assert!(diagram.contains(path), "missing {path}");
        }
    }

    #[test]
    fn architecture_diagram_fits_default_panel_height() {
        let app = app();
        let lines = architecture_diagram_strings(
            app.repo_model.as_ref(),
            app.impact_analysis.as_ref(),
            app.selected_future_index,
        );

        assert!(
            lines.len() <= 27,
            "architecture diagram has {} lines and clips inside the default 120x40 panel",
            lines.len()
        );
    }

    #[test]
    fn screen_transition_fx_area_is_full_frame() {
        let area = Rect::new(0, 0, 120, 40);

        assert_eq!(
            fx_area_for_screen(
                area,
                Screen::ImpactExplorer,
                AnimationStage::ImpactToFutures
            ),
            area
        );
        assert_eq!(
            fx_area_for_screen(
                area,
                Screen::FuturesCompare,
                AnimationStage::FuturesToImpact
            ),
            area
        );
        assert_eq!(
            fx_area_for_screen(
                area,
                Screen::ArchitectureScaffold,
                AnimationStage::DiagramReveal
            ),
            area
        );
    }

    #[test]
    fn repo_tree_screen_renders_selected_file_details() {
        let mut app = app();
        app.screen = Screen::RepoTree;
        app.selected_repo_file_index = 1;
        if let Some(repo) = &mut app.repo_model {
            if let Some(file) = repo.files.get_mut(1) {
                file.snippets = vec!["export function parseResume() {}".to_string()];
            }
        }
        let backend = TestBackend::new(120, 36);
        let mut terminal = Terminal::new(backend).unwrap();

        terminal
            .draw(|frame| render(frame, &mut app, StdDuration::from_millis(16)))
            .unwrap();

        let rendered = terminal.backend().to_string();
        assert!(rendered.contains("services/parser.ts"));
        assert!(rendered.contains("Kind: service"));
        assert!(rendered.contains("parseResume"));
    }

    #[test]
    fn screen_key_hints_match_active_handlers() {
        assert!(key_hints(Screen::ImpactExplorer).contains("g architecture"));
        assert!(key_hints(Screen::ImpactExplorer).contains("T repo tree"));
        assert!(key_hints(Screen::ImpactExplorer).contains("s sort"));
        assert!(key_hints(Screen::FileDetail).contains("s sort"));
        assert!(key_hints(Screen::ArchitectureScaffold).contains("T repo tree"));
        assert!(key_hints(Screen::ArchitectureScaffold).contains("-/+ zoom"));
        assert!(key_hints(Screen::ArchitectureScaffold).contains("h/j/k/l pan"));
        assert_eq!(key_hints(Screen::Input), "Enter scan | ? help | q quit");
        assert_eq!(key_hints(Screen::RepoScan), "? help | q quit");
    }

    #[test]
    fn impact_scores_accept_ten_point_scale() {
        assert_eq!(impact_score_out_of_10(10), 10);
        assert_eq!(impact_score_out_of_10(7), 7);
        assert_eq!(impact_score_out_of_10(82), 8);
        assert_eq!(impact_score_out_of_10(100), 10);
    }

    #[test]
    fn help_text_describes_architecture_shortcut() {
        let mut app = app();
        app.screen = Screen::Help;
        let backend = TestBackend::new(100, 28);
        let mut terminal = Terminal::new(backend).unwrap();

        terminal
            .draw(|frame| render(frame, &mut app, StdDuration::from_millis(16)))
            .unwrap();

        let rendered = terminal.backend().to_string();
        assert!(rendered.contains("g architecture"));
        assert!(!rendered.contains("g image blueprint status"));
    }
}
