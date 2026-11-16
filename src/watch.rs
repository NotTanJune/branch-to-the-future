use std::{
    env,
    path::{Path, PathBuf},
    process::Command,
    time::{Duration, Instant},
};

use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Wrap},
    Frame,
};
use tokio::sync::mpsc;

use crate::{
    ai::OpenAiClient,
    blast::{compute_blast, diff_fingerprint, git_changed_files, git_diff_text, untracked_list},
    blast::{BlastImpact, BlastReport, ChangedFile},
    cli::{dotenv_search_dirs_for, resolve_openai_api_key, validate_repo_input, WatchCommand},
    domain::{ImpactAnalysis, RepoModel, RiskLevel},
    machine::created_at_rfc3339,
    repo::scan_repo,
    repo_source::is_github_repo_link,
};

const DIFF_PROMPT_LIMIT: usize = 12_000;
const HELP: &str = "tab panel  j/k move  g/G ends  r rescan  a ai  q quit";
const NOTICE_SECS: u64 = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Panel {
    Changed,
    Blast,
    Summary,
}

impl Panel {
    pub fn next(self) -> Self {
        match self {
            Panel::Changed => Panel::Blast,
            Panel::Blast => Panel::Summary,
            Panel::Summary => Panel::Changed,
        }
    }

    pub fn prev(self) -> Self {
        match self {
            Panel::Changed => Panel::Summary,
            Panel::Blast => Panel::Changed,
            Panel::Summary => Panel::Blast,
        }
    }

    fn index(self) -> usize {
        match self {
            Panel::Changed => 0,
            Panel::Blast => 1,
            Panel::Summary => 2,
        }
    }
}

/// Moves a cursor by `delta` inside a list of `len` rows, clamped to the ends.
pub fn move_cursor(cursor: usize, len: usize, delta: isize) -> usize {
    if len == 0 {
        return 0;
    }
    let max = (len - 1) as isize;
    (cursor as isize + delta).clamp(0, max) as usize
}

#[derive(Debug, Clone)]
enum AiState {
    Idle,
    Running,
    Done(Box<ImpactAnalysis>),
    Failed(String),
}

enum WatchEvent {
    Ai(Result<ImpactAnalysis, String>),
}

struct WatchState {
    root: PathBuf,
    model: RepoModel,
    report: BlastReport,
    fingerprint: u64,
    updated: String,
    error: Option<String>,
    focus: Panel,
    cursor: [usize; 3],
    notice: Option<(String, Instant)>,
    ai: AiState,
    ai_fingerprint: Option<u64>,
}

pub async fn run_watch(command: WatchCommand) -> Result<()> {
    if is_github_repo_link(&command.repo_path) {
        anyhow::bail!("watch works on local repositories only");
    }
    validate_repo_input(&command.repo_path)?;
    let root = command.repo_path.canonicalize()?;
    let mut state = build_state(&root, &command)?;
    let mut terminal = ratatui::init();
    let result = event_loop(&mut terminal, &mut state, &command).await;
    ratatui::restore();
    result
}

fn build_state(root: &Path, command: &WatchCommand) -> Result<WatchState> {
    let (model, report, fingerprint) = rescan(root, command)?;
    Ok(WatchState {
        root: root.to_path_buf(),
        model,
        report,
        fingerprint,
        updated: clock_now(),
        error: None,
        focus: Panel::Blast,
        cursor: [0; 3],
        notice: None,
        ai: AiState::Idle,
        ai_fingerprint: None,
    })
}

fn rescan(root: &Path, command: &WatchCommand) -> Result<(RepoModel, BlastReport, u64)> {
    let fingerprint = diff_fingerprint(root)?;
    let model = scan_repo(root, command.max_file_bytes, &command.ignore)?;
    let changed = git_changed_files(root)?;
    let report = compute_blast(&model, &changed, created_at_rfc3339()?);
    Ok((model, report, fingerprint))
}

fn refresh(state: &mut WatchState, command: &WatchCommand) {
    match rescan(&state.root, command) {
        Ok((model, report, fingerprint)) => {
            state.model = model;
            state.report = report;
            state.fingerprint = fingerprint;
            state.updated = clock_now();
            state.error = None;
            for panel in [Panel::Changed, Panel::Blast, Panel::Summary] {
                let len = panel_len(state, panel);
                let cursor = &mut state.cursor[panel.index()];
                *cursor = move_cursor(*cursor, len, 0);
            }
        }
        Err(error) => state.error = Some(format!("{error:#}")),
    }
}

async fn event_loop(
    terminal: &mut ratatui::DefaultTerminal,
    state: &mut WatchState,
    command: &WatchCommand,
) -> Result<()> {
    let (tx, mut rx) = mpsc::unbounded_channel::<WatchEvent>();
    let interval = Duration::from_millis(command.interval_ms.max(100));
    let mut last_poll = Instant::now();

    loop {
        terminal.draw(|frame| render(frame, state))?;

        while let Ok(event) = rx.try_recv() {
            match event {
                WatchEvent::Ai(Ok(analysis)) => state.ai = AiState::Done(Box::new(analysis)),
                WatchEvent::Ai(Err(message)) => state.ai = AiState::Failed(message),
            }
        }

        if event::poll(Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Release {
                    continue;
                }
                match key.code {
                    KeyCode::Char('q') | KeyCode::Esc => return Ok(()),
                    KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        return Ok(())
                    }
                    KeyCode::Tab | KeyCode::Char('l') | KeyCode::Right => {
                        state.focus = state.focus.next()
                    }
                    KeyCode::BackTab | KeyCode::Char('h') | KeyCode::Left => {
                        state.focus = state.focus.prev()
                    }
                    KeyCode::Char('j') | KeyCode::Down => step_cursor(state, 1),
                    KeyCode::Char('k') | KeyCode::Up => step_cursor(state, -1),
                    KeyCode::PageDown => step_cursor(state, 10),
                    KeyCode::PageUp => step_cursor(state, -10),
                    KeyCode::Char('g') | KeyCode::Home => step_cursor(state, isize::MIN / 2),
                    KeyCode::Char('G') | KeyCode::End => step_cursor(state, isize::MAX / 2),
                    KeyCode::Char('r') => {
                        let before = state.fingerprint;
                        refresh(state, command);
                        last_poll = Instant::now();
                        let text = if state.error.is_some() {
                            "rescan failed"
                        } else if state.fingerprint == before {
                            "rescanned, no change"
                        } else {
                            "rescanned"
                        };
                        state.notice = Some((text.to_string(), Instant::now()));
                    }
                    KeyCode::Char('a') => start_ai(state, command, tx.clone()),
                    _ => {}
                }
            }
        }

        if last_poll.elapsed() >= interval {
            last_poll = Instant::now();
            let polled = diff_fingerprint(&state.root);
            if poll_needs_refresh(state.fingerprint, polled, &mut state.error) {
                refresh(state, command);
            }
        }
    }
}

/// Applies a fingerprint poll result: a successful read clears any stale error,
/// a failed read records it. Returns true when the diff changed and a rescan is due.
fn poll_needs_refresh(current: u64, polled: Result<u64>, error: &mut Option<String>) -> bool {
    match polled {
        Ok(fingerprint) => {
            *error = None;
            fingerprint != current
        }
        Err(err) => {
            *error = Some(format!("{err:#}"));
            false
        }
    }
}

fn panel_len(state: &WatchState, panel: Panel) -> usize {
    match panel {
        Panel::Changed => state.report.changed.len(),
        Panel::Blast => state.report.impacted.len(),
        Panel::Summary => {
            if matches!(state.ai, AiState::Idle) {
                summary_lines(&state.report, usize::MAX).len()
            } else {
                // AI text wraps; let the cursor run over a generous line count.
                64
            }
        }
    }
}

fn step_cursor(state: &mut WatchState, delta: isize) {
    let panel = state.focus;
    let len = panel_len(state, panel);
    let cursor = &mut state.cursor[panel.index()];
    *cursor = move_cursor(*cursor, len, delta);
}

fn panel_block(title: String, state: &WatchState, panel: Panel) -> Block<'static> {
    let style = if state.focus == panel {
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::DarkGray)
    };
    Block::default()
        .borders(Borders::ALL)
        .border_style(style)
        .title(title)
}

fn render_list(
    frame: &mut Frame,
    area: Rect,
    state: &WatchState,
    panel: Panel,
    title: String,
    lines: Vec<Line<'static>>,
) {
    let has_rows = !lines.is_empty();
    let list = List::new(lines.into_iter().map(ListItem::new))
        .block(panel_block(title, state, panel))
        .highlight_style(if state.focus == panel {
            Style::default().add_modifier(Modifier::REVERSED)
        } else {
            Style::default()
        });
    let mut list_state = ListState::default();
    if has_rows {
        list_state.select(Some(state.cursor[panel.index()]));
    }
    frame.render_stateful_widget(list, area, &mut list_state);
}

fn start_ai(state: &mut WatchState, command: &WatchCommand, tx: mpsc::UnboundedSender<WatchEvent>) {
    if matches!(state.ai, AiState::Running) {
        return;
    }
    let api_key = dotenv_search_dirs_for(&state.root)
        .and_then(|dirs| resolve_openai_api_key(env::var("OPENAI_API_KEY").ok().as_deref(), &dirs))
        .unwrap_or_default();
    if api_key.trim().is_empty() {
        state.ai = AiState::Failed("OPENAI_API_KEY not set".to_string());
        return;
    }
    state.ai = AiState::Running;
    state.ai_fingerprint = Some(state.fingerprint);

    let root = state.root.clone();
    let model = state.model.clone();
    let model_name =
        env::var("BRANCH_TO_THE_FUTURE_MODEL").unwrap_or_else(|_| command.text_model.clone());
    let effort = command.reasoning_effort;
    let (max_tokens, max_files) = (command.max_output_tokens, command.max_prompt_files);
    tokio::spawn(async move {
        let outcome = async {
            let diff = tokio::task::spawn_blocking({
                let root = root.clone();
                move || {
                    let diff = git_diff_text(&root)?;
                    anyhow::Ok(build_ai_prompt(&diff, &untracked_list(&root)))
                }
            })
            .await??;
            let client = match env::var("BRANCH_TO_THE_FUTURE_OPENAI_BASE_URL") {
                Ok(base) if !base.trim().is_empty() => {
                    OpenAiClient::with_base_url(api_key, base, model_name, effort)
                }
                _ => OpenAiClient::new(api_key, model_name, effort),
            }
            .with_limits(max_tokens, max_files);
            client.analyze(&model, &diff).await
        }
        .await;
        let _ = tx.send(WatchEvent::Ai(outcome.map_err(|e| format!("{e:#}"))));
    });
}

// ---------------------------------------------------------------- pure helpers

pub fn build_ai_prompt(diff: &str, untracked: &[String]) -> String {
    let mut prompt =
        String::from("Assess the blast radius of this working-tree diff before it is accepted:\n");
    prompt.push_str(&truncate_chars(diff, DIFF_PROMPT_LIMIT));
    if !untracked.is_empty() {
        prompt.push_str("\n\nUntracked files:\n");
        for path in untracked.iter().take(50) {
            prompt.push_str(path);
            prompt.push('\n');
        }
    }
    prompt
}

pub fn truncate_chars(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        text.to_string()
    } else {
        text.chars().take(max).collect()
    }
}

/// Keep the tail of `text` within `width` columns, marking the cut with an ellipsis.
pub fn truncate_left(text: &str, width: usize) -> String {
    let count = text.chars().count();
    if count <= width {
        return text.to_string();
    }
    if width == 0 {
        return String::new();
    }
    let tail: String = text.chars().skip(count - (width - 1)).collect();
    format!("…{tail}")
}

/// Keep the head of `text` within `width` columns, marking the cut with an ellipsis.
pub fn truncate_right(text: &str, width: usize) -> String {
    let count = text.chars().count();
    if count <= width {
        return text.to_string();
    }
    if width == 0 {
        return String::new();
    }
    let head: String = text.chars().take(width - 1).collect();
    format!("{head}…")
}

pub fn status_letter(status: &str) -> char {
    match status {
        "modified" => 'M',
        "added" => 'A',
        "deleted" => 'D',
        "renamed" => 'R',
        "untracked" => '?',
        "proposed" => 'P',
        _ => '-',
    }
}

fn level_style(level: &str) -> Style {
    let color = match level {
        "high" => Color::Red,
        "medium" => Color::Yellow,
        _ => Color::Green,
    };
    Style::default()
        .fg(Color::Black)
        .bg(color)
        .add_modifier(Modifier::BOLD)
}

fn risk_color(risk: &RiskLevel) -> Color {
    match risk {
        RiskLevel::Low => Color::Green,
        RiskLevel::Medium => Color::Yellow,
        RiskLevel::High => Color::Red,
    }
}

fn clock_now() -> String {
    let local = Command::new("date")
        .arg("+%H:%M:%S")
        .output()
        .ok()
        .filter(|out| out.status.success())
        .map(|out| String::from_utf8_lossy(&out.stdout).trim().to_string())
        .filter(|text| !text.is_empty());
    local.unwrap_or_else(|| {
        let now = time::OffsetDateTime::now_utc();
        format!("{:02}:{:02}:{:02}", now.hour(), now.minute(), now.second())
    })
}

fn join_limited(items: &[String], none: &str) -> String {
    if items.is_empty() {
        none.to_string()
    } else {
        items.join(", ")
    }
}

// ---------------------------------------------------------------- rendering

fn render(frame: &mut Frame, state: &WatchState) {
    let area = frame.area();
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Min(5),
            Constraint::Length(bottom_height(area.height)),
            Constraint::Length(1),
        ])
        .split(area);
    render_header(frame, rows[0], state);

    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(35), Constraint::Percentage(65)])
        .split(rows[1]);
    render_changed(frame, cols[0], state);
    render_blast(frame, cols[1], state);

    match &state.ai {
        AiState::Idle => render_summary(frame, rows[2], state),
        _ => render_ai(frame, rows[2], state),
    }
    frame.render_widget(
        Paragraph::new(Span::styled(HELP, Style::default().fg(Color::DarkGray))),
        rows[3],
    );
}

fn bottom_height(total: u16) -> u16 {
    (total / 3).clamp(6, 12)
}

fn render_header(frame: &mut Frame, area: Rect, state: &WatchState) {
    let report = &state.report;
    let mut spans = vec![
        Span::styled(
            truncate_right(&state.model.repo_name, 24),
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(" "),
        Span::styled(
            format!(" {} ", report.level.to_uppercase()),
            level_style(&report.level),
        ),
        Span::raw(format!(
            " {}/10  {} changed  updated {}",
            report.score,
            report.changed.len(),
            state.updated
        )),
    ];
    if let Some((text, at)) = &state.notice {
        if at.elapsed() < Duration::from_secs(NOTICE_SECS) {
            spans.push(Span::styled(
                format!("  {text}"),
                Style::default().fg(Color::Green),
            ));
        }
    }
    if let Some(error) = &state.error {
        spans.push(Span::styled(
            format!("  ! {}", truncate_right(error, 30)),
            Style::default().fg(Color::Red),
        ));
    }
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn render_changed(frame: &mut Frame, area: Rect, state: &WatchState) {
    let title = format!("Changed ({})", state.report.changed.len());
    let inner_width = area.width.saturating_sub(2) as usize;
    if state.report.changed.is_empty() {
        let empty = Line::styled(
            "no working-tree changes",
            Style::default().fg(Color::DarkGray),
        );
        let block = panel_block(title, state, Panel::Changed);
        frame.render_widget(Paragraph::new(empty).block(block), area);
        return;
    }
    let lines = state
        .report
        .changed
        .iter()
        .map(|change| changed_line(change, inner_width))
        .collect();
    render_list(frame, area, state, Panel::Changed, title, lines);
}

fn changed_line(change: &ChangedFile, width: usize) -> Line<'static> {
    let letter = status_letter(&change.status);
    let color = match letter {
        'A' | '?' | 'P' => Color::Green,
        'D' => Color::Red,
        _ => Color::Yellow,
    };
    Line::from(vec![
        Span::styled(format!("{letter} "), Style::default().fg(color)),
        Span::raw(truncate_left(&change.path, width.saturating_sub(2))),
    ])
}

fn render_blast(frame: &mut Frame, area: Rect, state: &WatchState) {
    let impacted = &state.report.impacted;
    let title = format!("Blast radius ({})", impacted.len());
    let inner_width = area.width.saturating_sub(2) as usize;
    if impacted.is_empty() {
        let empty = Line::styled(
            "nothing imports the changed files",
            Style::default().fg(Color::DarkGray),
        );
        let block = panel_block(title, state, Panel::Blast);
        frame.render_widget(Paragraph::new(empty).block(block), area);
        return;
    }
    let lines = impacted
        .iter()
        .map(|item| blast_line(item, inner_width))
        .collect();
    render_list(frame, area, state, Panel::Blast, title, lines);
}

fn blast_line(item: &BlastImpact, width: usize) -> Line<'static> {
    let marker = if item.is_test { "t" } else { " " };
    let prefix = format!("{}{} ", item.depth, marker);
    let body = width.saturating_sub(prefix.chars().count());
    let path_budget = (body * 3 / 5).max(8).min(body);
    let path = truncate_left(&item.path, path_budget);
    let via_budget = body.saturating_sub(path.chars().count() + 4);
    let mut spans = vec![
        Span::styled(
            prefix,
            Style::default().fg(if item.depth == 1 {
                Color::Red
            } else {
                Color::Yellow
            }),
        ),
        Span::raw(path),
    ];
    if via_budget >= 4 {
        let via = item.via.join(", ");
        spans.push(Span::styled(
            format!(" <- {}", truncate_left(&via, via_budget)),
            Style::default().fg(Color::DarkGray),
        ));
    }
    Line::from(spans)
}

fn summary_lines(report: &BlastReport, width: usize) -> Vec<Line<'static>> {
    let label = |text: &str| {
        Span::styled(
            format!("{text}: "),
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )
    };
    let mut lines = vec![Line::from(label("Reasons"))];
    for reason in &report.reasons {
        lines.push(Line::raw(format!(
            " - {}",
            truncate_right(reason, width.saturating_sub(3))
        )));
    }
    let single = |name: &str, value: String| {
        let budget = width.saturating_sub(name.len() + 2);
        Line::from(vec![label(name), Span::raw(truncate_right(&value, budget))])
    };
    lines.push(single("Tests", join_limited(&report.tests, "none")));
    lines.push(single(
        "Untested",
        join_limited(&report.untested_changes, "none"),
    ));
    let routes: Vec<String> = report
        .routes
        .iter()
        .map(|r| format!("{} {}", r.method, r.route))
        .collect();
    lines.push(single("Routes", join_limited(&routes, "none")));
    lines.push(single("Sensitive", join_limited(&report.sensitive, "none")));
    lines
}

fn render_summary(frame: &mut Frame, area: Rect, state: &WatchState) {
    let width = area.width.saturating_sub(2) as usize;
    let lines = summary_lines(&state.report, width);
    render_list(
        frame,
        area,
        state,
        Panel::Summary,
        "Summary".to_string(),
        lines,
    );
}

fn render_ai(frame: &mut Frame, area: Rect, state: &WatchState) {
    let stale = state
        .ai_fingerprint
        .map(|fp| fp != state.fingerprint)
        .unwrap_or(false);
    let title = if stale && !matches!(state.ai, AiState::Running) {
        "AI analysis (stale)"
    } else {
        "AI analysis"
    };
    let block = panel_block(title.to_string(), state, Panel::Summary);
    let width = area.width.saturating_sub(2) as usize;
    let lines: Vec<Line> = match &state.ai {
        AiState::Idle => Vec::new(),
        AiState::Running => vec![Line::styled(
            "analyzing...",
            Style::default().fg(Color::Yellow),
        )],
        AiState::Failed(message) => vec![Line::styled(
            truncate_right(message, width),
            Style::default().fg(Color::Red),
        )],
        AiState::Done(analysis) => {
            let mut lines = vec![Line::raw(analysis.summary.clone())];
            let mut files: Vec<_> = analysis.impact_path.iter().collect();
            files.sort_by_key(|file| std::cmp::Reverse(file.impact_score));
            for file in files.into_iter().take(5) {
                lines.push(Line::from(vec![
                    Span::raw(format!("{:>2} ", file.impact_score)),
                    Span::styled(
                        format!("{:<6}", file.risk.to_string()),
                        Style::default().fg(risk_color(&file.risk)),
                    ),
                    Span::raw(truncate_left(&file.path, width.saturating_sub(10))),
                ]));
            }
            for risk in analysis.risk_summary.iter().take(3) {
                lines.push(Line::styled(
                    format!("! {risk}"),
                    Style::default().fg(Color::Gray),
                ));
            }
            lines
        }
    };
    frame.render_widget(
        Paragraph::new(lines)
            .block(block)
            .wrap(Wrap { trim: true })
            .scroll((state.cursor[Panel::Summary.index()] as u16, 0)),
        area,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn poll_clears_stale_error_on_success() {
        let mut error = Some("boom".to_string());
        assert!(!poll_needs_refresh(5, Ok(5), &mut error));
        assert!(error.is_none(), "unchanged fingerprint must clear error");

        let mut error = Some("boom".to_string());
        assert!(poll_needs_refresh(5, Ok(6), &mut error));
        assert!(error.is_none());

        let mut error = None;
        assert!(!poll_needs_refresh(
            5,
            Err(anyhow::anyhow!("git died")),
            &mut error
        ));
        assert!(error.as_deref().unwrap().contains("git died"));
    }

    #[test]
    fn truncates_from_left_with_ellipsis() {
        assert_eq!(truncate_left("src/a/b/c.rs", 20), "src/a/b/c.rs");
        let cut = truncate_left("src/very/long/path/file.rs", 10);
        assert_eq!(cut.chars().count(), 10);
        assert!(cut.starts_with('…') && cut.ends_with("file.rs"));
        assert_eq!(truncate_left("abc", 0), "");
        assert_eq!(truncate_right("abcdef", 4), "abc…");
    }

    #[test]
    fn status_letters_cover_all_statuses() {
        let pairs = [
            ("modified", 'M'),
            ("added", 'A'),
            ("deleted", 'D'),
            ("renamed", 'R'),
            ("untracked", '?'),
            ("proposed", 'P'),
        ];
        for (status, letter) in pairs {
            assert_eq!(status_letter(status), letter);
        }
    }

    #[test]
    fn ai_prompt_truncates_diff_and_lists_untracked() {
        let diff = "x".repeat(DIFF_PROMPT_LIMIT + 500);
        let prompt = build_ai_prompt(&diff, &["new.rs".to_string()]);
        assert!(prompt.starts_with(
            "Assess the blast radius of this working-tree diff before it is accepted:\n"
        ));
        assert!(prompt.contains("new.rs"));
        assert!(prompt.matches('x').count() == DIFF_PROMPT_LIMIT);
    }

    #[test]
    fn cursor_moves_and_clamps() {
        assert_eq!(move_cursor(0, 5, 1), 1);
        assert_eq!(move_cursor(4, 5, 1), 4);
        assert_eq!(move_cursor(0, 5, -1), 0);
        assert_eq!(move_cursor(3, 5, isize::MIN / 2), 0);
        assert_eq!(move_cursor(0, 5, isize::MAX / 2), 4);
        assert_eq!(move_cursor(7, 0, 1), 0);
        assert_eq!(move_cursor(9, 3, 0), 2);
    }

    #[test]
    fn panel_focus_cycles_both_ways() {
        assert_eq!(Panel::Changed.next(), Panel::Blast);
        assert_eq!(Panel::Summary.next(), Panel::Changed);
        assert_eq!(Panel::Changed.prev(), Panel::Summary);
        assert_eq!(Panel::Blast.prev().next(), Panel::Blast);
    }

    #[test]
    fn blast_line_fits_narrow_width() {
        let item = BlastImpact {
            path: "src/some/deeply/nested/module/file.ts".to_string(),
            depth: 2,
            via: vec!["src/another/long/path/changed.ts".to_string()],
            is_test: true,
        };
        let line = blast_line(&item, 40);
        let width: usize = line.spans.iter().map(|s| s.content.chars().count()).sum();
        assert!(width <= 40, "line width {width}");
    }
}
