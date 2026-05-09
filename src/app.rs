use std::{
    io,
    path::PathBuf,
    time::{Duration, Instant},
};

use anyhow::Result;
use crossterm::{
    event::{self, Event as CrosstermEvent, KeyCode, KeyEvent, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};
use tachyonfx::EffectManager;
use tokio::{sync::mpsc, task};
use uuid::Uuid;

use crate::{
    ai::{AiStreamEvent, OpenAiClient},
    artifacts::export_markdown,
    cli::{Cli, ReasoningEffort},
    domain::{
        AnimationStage, ArchitectureZoom, ImpactAnalysis, ImpactFile, ImpactSort, RepoModel, Screen,
    },
    repo::scan_repo,
    repo_source::PreparedRepo,
    ui,
};

const SCREEN_TRANSITION_RESET_TICKS: usize = 10;

pub struct App {
    pub repo_path: PathBuf,
    pub output_dir: PathBuf,
    pub max_file_bytes: u64,
    pub ignore: Vec<String>,
    pub api_key: String,
    pub text_model: String,
    pub reasoning_effort: ReasoningEffort,
    pub max_output_tokens: u32,
    pub max_prompt_files: usize,
    pub session_id: String,
    pub change_request: String,
    pub screen: Screen,
    pub previous_screen: Screen,
    pub repo_model: Option<RepoModel>,
    pub impact_analysis: Option<ImpactAnalysis>,
    pub selected_file_index: usize,
    pub selected_repo_file_index: usize,
    pub selected_future_index: usize,
    pub impact_sort: ImpactSort,
    pub architecture_scroll_x: u16,
    pub architecture_scroll_y: u16,
    pub architecture_zoom: ArchitectureZoom,
    pub repo_tree_return_screen: Screen,
    pub architecture_return_screen: Screen,
    pub status: String,
    pub error: Option<String>,
    pub export_path: Option<PathBuf>,
    pub animation_stage: AnimationStage,
    pub trace_revealed: usize,
    pub stream_preview: String,
    pub cursor_visible: bool,
    pub spinner_frame: usize,
    pub stage_ticks: usize,
    pub effects: EffectManager<String>,
}

enum AppEvent {
    ScanComplete(RepoModel),
    AnalysisProgress(String),
    AnalysisComplete(ImpactAnalysis),
    Error(String),
}

impl App {
    pub fn from_cli(cli: Cli, api_key: String) -> Self {
        let prepared = PreparedRepo::local(cli.repo_path.clone());
        Self::from_prepared_cli(cli, api_key, prepared)
    }

    pub fn from_prepared_cli(cli: Cli, api_key: String, prepared_repo: PreparedRepo) -> Self {
        let text_model = cli.resolved_model();
        let output_dir = cli.resolved_output_dir_for(&prepared_repo.local_path);
        let mut app = Self {
            repo_path: prepared_repo.local_path,
            output_dir,
            max_file_bytes: cli.max_file_bytes,
            ignore: cli.ignore,
            api_key,
            text_model,
            reasoning_effort: cli.reasoning_effort,
            max_output_tokens: cli.max_output_tokens,
            max_prompt_files: cli.max_prompt_files,
            session_id: Uuid::new_v4().to_string(),
            change_request: String::new(),
            screen: Screen::Input,
            previous_screen: Screen::Input,
            repo_model: None,
            impact_analysis: None,
            selected_file_index: 0,
            selected_repo_file_index: 0,
            selected_future_index: 0,
            impact_sort: ImpactSort::HighToLow,
            architecture_scroll_x: 0,
            architecture_scroll_y: 0,
            architecture_zoom: ArchitectureZoom::Normal,
            repo_tree_return_screen: Screen::ImpactExplorer,
            architecture_return_screen: Screen::ImpactExplorer,
            status: "Enter change request".to_string(),
            error: None,
            export_path: None,
            animation_stage: AnimationStage::BootReveal,
            trace_revealed: 0,
            stream_preview: String::new(),
            cursor_visible: true,
            spinner_frame: 0,
            stage_ticks: 0,
            effects: EffectManager::default(),
        };
        app.set_stage(AnimationStage::BootReveal);
        app
    }

    pub async fn run(mut self) -> Result<()> {
        enable_raw_mode()?;
        let mut stdout = io::stdout();
        execute!(stdout, EnterAlternateScreen)?;
        let backend = CrosstermBackend::new(stdout);
        let mut terminal = Terminal::new(backend)?;
        let result = self.run_loop(&mut terminal).await;
        disable_raw_mode()?;
        execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
        terminal.show_cursor()?;
        result
    }

    async fn run_loop(
        &mut self,
        terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    ) -> Result<()> {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let mut last_tick = Instant::now();
        let mut last_frame = Instant::now();

        loop {
            let frame_delta = last_frame.elapsed();
            last_frame = Instant::now();
            terminal.draw(|frame| ui::render(frame, self, frame_delta))?;

            while let Ok(app_event) = rx.try_recv() {
                self.handle_app_event(app_event, tx.clone());
            }

            let timeout = Duration::from_millis(40)
                .saturating_sub(last_tick.elapsed())
                .max(Duration::from_millis(1));
            if event::poll(timeout)? {
                if let CrosstermEvent::Key(key) = event::read()? {
                    if self.handle_key(key, tx.clone()) {
                        break;
                    }
                }
            }
            if last_tick.elapsed() >= Duration::from_millis(180) {
                self.tick();
                last_tick = Instant::now();
            }
        }
        Ok(())
    }

    fn handle_app_event(&mut self, event: AppEvent, tx: mpsc::UnboundedSender<AppEvent>) {
        match event {
            AppEvent::ScanComplete(repo_model) => {
                self.status = format!(
                    "Scanned {} files. Streaming OpenAI {}.",
                    repo_model.files.len(),
                    self.text_model
                );
                self.repo_model = Some(repo_model.clone());
                self.clamp_repo_file_selection();
                self.set_stage(AnimationStage::ScanningSweep);
                self.spawn_analysis(repo_model, tx);
            }
            AppEvent::AnalysisProgress(delta) => {
                self.status = "OpenAI streaming structured impact JSON".to_string();
                self.push_stream_delta(&delta);
                if self.screen == Screen::RepoScan
                    && self.animation_stage != AnimationStage::StreamShimmer
                {
                    self.set_stage(AnimationStage::StreamShimmer);
                }
            }
            AppEvent::AnalysisComplete(analysis) => {
                self.status = "Impact analysis ready".to_string();
                self.trace_revealed = 1.min(analysis.impact_path.len());
                self.impact_analysis = Some(analysis);
                self.selected_file_index = self.selected_file_index.min(
                    self.impact_analysis
                        .as_ref()
                        .map(|analysis| analysis.impact_path.len().saturating_sub(1))
                        .unwrap_or(0),
                );
                self.screen = Screen::ImpactExplorer;
                self.set_stage(AnimationStage::ImpactTrace);
            }
            AppEvent::Error(error) => {
                self.error = Some(error.clone());
                self.status = error;
                self.screen = Screen::Error;
            }
        }
    }

    fn handle_key(&mut self, key: KeyEvent, tx: mpsc::UnboundedSender<AppEvent>) -> bool {
        if matches!(key.code, KeyCode::Char('q')) {
            return true;
        }
        if matches!(key.code, KeyCode::Char('?')) {
            self.previous_screen = self.screen;
            self.screen = Screen::Help;
            return false;
        }
        if self.handle_global_analysis_key(&key) {
            return false;
        }

        match self.screen {
            Screen::Input => self.handle_input_key(key, tx),
            Screen::RepoScan => {}
            Screen::ImpactExplorer => self.handle_explorer_key(key, tx),
            Screen::FileDetail => self.handle_detail_key(key),
            Screen::FuturesCompare => self.handle_futures_key(key),
            Screen::RepoTree => self.handle_repo_tree_key(key),
            Screen::ArchitectureScaffold => self.handle_architecture_key(key),
            Screen::ExportSummary | Screen::Error | Screen::Help => self.handle_modal_key(key),
        }
        false
    }

    fn handle_global_analysis_key(&mut self, key: &KeyEvent) -> bool {
        match key.code {
            KeyCode::Char('T') if repo_tree_shortcut_active(self.screen) => {
                self.open_repo_tree();
                true
            }
            KeyCode::Char('g') if architecture_shortcut_active(self.screen) => {
                self.show_architecture();
                true
            }
            _ => false,
        }
    }

    fn handle_input_key(&mut self, key: KeyEvent, tx: mpsc::UnboundedSender<AppEvent>) {
        match key.code {
            KeyCode::Char(c)
                if key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT =>
            {
                self.change_request.push(c);
            }
            KeyCode::Backspace => {
                self.change_request.pop();
            }
            KeyCode::Enter if !self.change_request.trim().is_empty() => {
                self.screen = Screen::RepoScan;
                self.status = "Scanning repository".to_string();
                self.stream_preview.clear();
                self.set_stage(AnimationStage::RepoMaterialize);
                self.spawn_scan(tx);
            }
            _ => {}
        }
    }

    fn handle_explorer_key(&mut self, key: KeyEvent, _tx: mpsc::UnboundedSender<AppEvent>) {
        match key.code {
            KeyCode::Tab | KeyCode::BackTab => {
                self.screen = Screen::FuturesCompare;
                self.restart_stage(AnimationStage::ImpactToFutures);
            }
            KeyCode::Enter => self.screen = Screen::FileDetail,
            KeyCode::Down | KeyCode::Char('j') => self.bump_file(1),
            KeyCode::Up | KeyCode::Char('k') => self.bump_file(-1),
            KeyCode::Char('r') => {
                self.trace_revealed = 0;
                self.restart_stage(AnimationStage::ReplayTrace);
                self.status = "Replaying impact trace".to_string();
            }
            KeyCode::Char('g') => self.show_architecture(),
            KeyCode::Char('e') => self.export(),
            KeyCode::Char('s') => self.cycle_impact_sort(),
            KeyCode::Char('p') => {
                self.status = "Patch skeleton included in Markdown export".to_string()
            }
            KeyCode::Char('t') => self.status = "Test plan included in Markdown export".to_string(),
            _ => {}
        }
    }

    fn handle_detail_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => self.screen = Screen::ImpactExplorer,
            KeyCode::Down | KeyCode::Char('j') => self.bump_file(1),
            KeyCode::Up | KeyCode::Char('k') => self.bump_file(-1),
            KeyCode::Char('s') => self.cycle_impact_sort(),
            _ => {}
        }
    }

    fn handle_futures_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc | KeyCode::Tab | KeyCode::BackTab => {
                self.screen = Screen::ImpactExplorer;
                self.restart_stage(AnimationStage::FuturesToImpact);
            }
            KeyCode::Down | KeyCode::Char('j') => self.bump_future(1),
            KeyCode::Up | KeyCode::Char('k') => self.bump_future(-1),
            KeyCode::Char('e') => self.export(),
            _ => {}
        }
    }

    fn handle_repo_tree_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => self.screen = self.repo_tree_return_screen,
            KeyCode::Down | KeyCode::Char('j') => self.bump_repo_file(1),
            KeyCode::Up | KeyCode::Char('k') => self.bump_repo_file(-1),
            KeyCode::Char('g') => self.show_architecture(),
            KeyCode::Char('e') => self.export(),
            _ => {}
        }
    }

    fn handle_architecture_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => self.screen = self.architecture_return_screen,
            KeyCode::Char('e') => self.export(),
            KeyCode::Down | KeyCode::Char('j') => self.scroll_architecture(0, 1),
            KeyCode::Up | KeyCode::Char('k') => self.scroll_architecture(0, -1),
            KeyCode::Right | KeyCode::Char('l') => self.scroll_architecture(8, 0),
            KeyCode::Left | KeyCode::Char('h') => self.scroll_architecture(-8, 0),
            KeyCode::PageDown => self.scroll_architecture(0, 10),
            KeyCode::PageUp => self.scroll_architecture(0, -10),
            KeyCode::Home => {
                self.architecture_scroll_x = 0;
                self.architecture_scroll_y = 0;
                self.status = "Architecture pan reset".to_string();
            }
            KeyCode::Char('-') => self.set_architecture_zoom(self.architecture_zoom.zoom_out()),
            KeyCode::Char('+') | KeyCode::Char('=') => {
                self.set_architecture_zoom(self.architecture_zoom.zoom_in())
            }
            KeyCode::Char('0') => {
                self.architecture_scroll_x = 0;
                self.architecture_scroll_y = 0;
                self.set_architecture_zoom(ArchitectureZoom::Normal);
            }
            _ => {}
        }
    }

    fn handle_modal_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc | KeyCode::Enter => {
                self.screen = match self.screen {
                    Screen::Help => self.previous_screen,
                    Screen::Error => Screen::Input,
                    _ => Screen::ImpactExplorer,
                };
            }
            KeyCode::Char('r') if self.screen == Screen::Error => {
                self.error = None;
                self.screen = Screen::Input;
                self.status = "Edit request and retry".to_string();
            }
            _ => {}
        }
    }

    fn spawn_scan(&self, tx: mpsc::UnboundedSender<AppEvent>) {
        let repo_path = self.repo_path.clone();
        let max_file_bytes = self.max_file_bytes;
        let ignore = self.ignore.clone();
        task::spawn_blocking(move || scan_repo(&repo_path, max_file_bytes, &ignore)).then_send(
            tx,
            AppEvent::ScanComplete,
            AppEvent::Error,
        );
    }

    fn spawn_analysis(&self, repo_model: RepoModel, tx: mpsc::UnboundedSender<AppEvent>) {
        let client = OpenAiClient::new(
            self.api_key.clone(),
            self.text_model.clone(),
            self.reasoning_effort,
        )
        .with_limits(self.max_output_tokens, self.max_prompt_files);
        let change_request = self.change_request.clone();
        tokio::spawn(async move {
            let progress_tx = tx.clone();
            let event = match client
                .analyze_streaming(&repo_model, &change_request, move |event| match event {
                    AiStreamEvent::Created => {
                        let _ = progress_tx
                            .send(AppEvent::AnalysisProgress("response.created".to_string()));
                    }
                    AiStreamEvent::TextDelta(delta) => {
                        let _ = progress_tx.send(AppEvent::AnalysisProgress(delta));
                    }
                    AiStreamEvent::Completed => {
                        let _ = progress_tx
                            .send(AppEvent::AnalysisProgress("response.completed".to_string()));
                    }
                })
                .await
            {
                Ok(analysis) => AppEvent::AnalysisComplete(analysis),
                Err(error) => AppEvent::Error(error.to_string()),
            };
            let _ = tx.send(event);
        });
    }

    fn bump_file(&mut self, delta: isize) {
        let len = self
            .impact_analysis
            .as_ref()
            .map(|analysis| analysis.impact_path.len())
            .unwrap_or(0);
        if len == 0 {
            return;
        }
        self.selected_file_index = wrap(self.selected_file_index, len, delta);
    }

    fn cycle_impact_sort(&mut self) {
        let selected_path = self
            .selected_impact_file()
            .map(|file| file.path.clone())
            .unwrap_or_default();
        self.impact_sort = self.impact_sort.next();
        if let Some(index) = self
            .ordered_impact_indices()
            .into_iter()
            .position(|file_index| {
                self.impact_analysis
                    .as_ref()
                    .and_then(|analysis| analysis.impact_path.get(file_index))
                    .map(|file| file.path == selected_path)
                    .unwrap_or(false)
            })
        {
            self.selected_file_index = index;
            self.trace_revealed = self.trace_revealed.max(index + 1);
        }
        self.status = format!("Impact sort: {}", self.impact_sort);
    }

    fn bump_future(&mut self, delta: isize) {
        let len = self
            .impact_analysis
            .as_ref()
            .map(|analysis| analysis.futures.len())
            .unwrap_or(0);
        if len == 0 {
            return;
        }
        self.selected_future_index = wrap(self.selected_future_index, len, delta);
    }

    fn bump_repo_file(&mut self, delta: isize) {
        let len = self
            .repo_model
            .as_ref()
            .map(|repo| repo.files.len())
            .unwrap_or(0);
        if len == 0 {
            return;
        }
        self.selected_repo_file_index = wrap(self.selected_repo_file_index, len, delta);
    }

    fn scroll_architecture(&mut self, dx: i16, dy: i16) {
        self.architecture_scroll_x = offset_u16(self.architecture_scroll_x, dx);
        self.architecture_scroll_y = offset_u16(self.architecture_scroll_y, dy);
        self.status = format!(
            "Architecture pan row {}, col {}",
            self.architecture_scroll_y, self.architecture_scroll_x
        );
    }

    fn set_architecture_zoom(&mut self, zoom: ArchitectureZoom) {
        self.architecture_zoom = zoom;
        self.architecture_scroll_x = 0;
        self.architecture_scroll_y = 0;
        self.status = format!("Architecture zoom: {zoom}");
    }

    fn open_repo_tree(&mut self) {
        if self.repo_model.is_none() {
            self.status = "Scan repository before opening repo tree".to_string();
            return;
        }
        if self.screen != Screen::RepoTree {
            self.repo_tree_return_screen = self.screen;
            self.sync_repo_selection_from_impact();
        }
        self.clamp_repo_file_selection();
        self.status = "Repo tree opened".to_string();
        self.screen = Screen::RepoTree;
    }

    fn sync_repo_selection_from_impact(&mut self) {
        let selected_path = self.selected_impact_file().map(|file| file.path.as_str());
        let Some(path) = selected_path else {
            return;
        };
        if let Some(index) = self
            .repo_model
            .as_ref()
            .and_then(|repo| repo.files.iter().position(|file| file.path == path))
        {
            self.selected_repo_file_index = index;
        }
    }

    fn clamp_repo_file_selection(&mut self) {
        let len = self
            .repo_model
            .as_ref()
            .map(|repo| repo.files.len())
            .unwrap_or(0);
        if len == 0 {
            self.selected_repo_file_index = 0;
        } else {
            self.selected_repo_file_index = self.selected_repo_file_index.min(len - 1);
        }
    }

    fn export(&mut self) {
        let (Some(repo), Some(analysis)) = (&self.repo_model, &self.impact_analysis) else {
            self.status = "No analysis to export yet".to_string();
            return;
        };
        match export_markdown(
            &self.output_dir,
            &self.session_id,
            &self.change_request,
            &repo.repo_name,
            analysis,
            self.selected_future_index,
        ) {
            Ok(path) => {
                self.export_path = Some(path.clone());
                self.status = format!("Exported {}", path.display());
                self.screen = Screen::ExportSummary;
            }
            Err(error) => {
                self.error = Some(error.to_string());
                self.screen = Screen::Error;
            }
        }
    }

    fn tick(&mut self) {
        self.cursor_visible = !self.cursor_visible;
        self.spinner_frame = self.spinner_frame.wrapping_add(1);
        self.stage_ticks = self.stage_ticks.wrapping_add(1);
        if self.stage_ticks >= SCREEN_TRANSITION_RESET_TICKS
            && is_screen_transition(self.animation_stage)
        {
            self.animation_stage = AnimationStage::LockIn;
            return;
        }
        if let Some(analysis) = &self.impact_analysis {
            if self.trace_revealed < analysis.impact_path.len() {
                self.trace_revealed += 1;
            } else if self.animation_stage == AnimationStage::ImpactTrace {
                self.animation_stage = AnimationStage::RiskBloom;
            } else if self.animation_stage == AnimationStage::RiskBloom {
                self.animation_stage = AnimationStage::LockIn;
            }
        }
    }

    fn set_stage(&mut self, stage: AnimationStage) {
        if self.animation_stage == stage {
            return;
        }
        self.restart_stage(stage);
    }

    fn restart_stage(&mut self, stage: AnimationStage) {
        self.animation_stage = stage;
        self.stage_ticks = 0;
        self.effects
            .add_unique_effect("stage".to_string(), crate::fx::stage_effect(stage));
    }

    fn push_stream_delta(&mut self, delta: &str) {
        if delta.starts_with("response.") {
            self.stream_preview = delta.to_string();
            return;
        }
        self.stream_preview.push_str(delta);
        if self.stream_preview.len() > 900 {
            let start = self.stream_preview.len().saturating_sub(900);
            self.stream_preview = self.stream_preview[start..].to_string();
        }
    }

    fn show_architecture(&mut self) {
        if self.impact_analysis.is_none() {
            self.status = "Run impact analysis before opening architecture scaffold".to_string();
            return;
        }
        if self.screen != Screen::ArchitectureScaffold {
            self.architecture_return_screen = self.screen;
        }
        self.status = "Architecture scaffold opened".to_string();
        self.screen = Screen::ArchitectureScaffold;
        self.restart_stage(AnimationStage::DiagramReveal);
    }
}

impl App {
    pub fn ordered_impact_indices(&self) -> Vec<usize> {
        let Some(analysis) = &self.impact_analysis else {
            return Vec::new();
        };
        let mut indices = (0..analysis.impact_path.len()).collect::<Vec<_>>();
        match self.impact_sort {
            ImpactSort::HighToLow => indices.sort_by(|left, right| {
                impact_score_out_of_10(analysis.impact_path[*right].impact_score)
                    .cmp(&impact_score_out_of_10(
                        analysis.impact_path[*left].impact_score,
                    ))
                    .then_with(|| left.cmp(right))
            }),
            ImpactSort::LowToHigh => indices.sort_by(|left, right| {
                impact_score_out_of_10(analysis.impact_path[*left].impact_score)
                    .cmp(&impact_score_out_of_10(
                        analysis.impact_path[*right].impact_score,
                    ))
                    .then_with(|| left.cmp(right))
            }),
            ImpactSort::ModelOrder => {}
        }
        indices
    }

    pub fn visible_impact_indices(&self) -> Vec<usize> {
        self.ordered_impact_indices()
            .into_iter()
            .take(self.trace_revealed)
            .collect()
    }

    pub fn selected_impact_file(&self) -> Option<&ImpactFile> {
        let file_index = self
            .ordered_impact_indices()
            .get(self.selected_file_index)
            .copied()?;
        self.impact_analysis
            .as_ref()
            .and_then(|analysis| analysis.impact_path.get(file_index))
    }
}

pub fn impact_score_out_of_10(score: u8) -> u8 {
    if score <= 10 {
        score
    } else {
        ((score as f32 / 10.0).round() as u8).clamp(0, 10)
    }
}

trait ThenSend<T> {
    fn then_send(
        self,
        tx: mpsc::UnboundedSender<AppEvent>,
        ok: fn(T) -> AppEvent,
        err: fn(String) -> AppEvent,
    );
}

impl<T> ThenSend<T> for task::JoinHandle<Result<T>>
where
    T: Send + 'static,
{
    fn then_send(
        self,
        tx: mpsc::UnboundedSender<AppEvent>,
        ok: fn(T) -> AppEvent,
        err: fn(String) -> AppEvent,
    ) {
        tokio::spawn(async move {
            let event = match self.await {
                Ok(Ok(value)) => ok(value),
                Ok(Err(error)) => err(error.to_string()),
                Err(error) => err(error.to_string()),
            };
            let _ = tx.send(event);
        });
    }
}

fn wrap(current: usize, len: usize, delta: isize) -> usize {
    ((current as isize + delta).rem_euclid(len as isize)) as usize
}

fn offset_u16(current: u16, delta: i16) -> u16 {
    if delta.is_negative() {
        current.saturating_sub(delta.unsigned_abs())
    } else {
        current.saturating_add(delta as u16)
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

fn repo_tree_shortcut_active(screen: Screen) -> bool {
    matches!(
        screen,
        Screen::ImpactExplorer
            | Screen::FileDetail
            | Screen::FuturesCompare
            | Screen::ArchitectureScaffold
    )
}

fn architecture_shortcut_active(screen: Screen) -> bool {
    matches!(
        screen,
        Screen::ImpactExplorer | Screen::FileDetail | Screen::FuturesCompare | Screen::RepoTree
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::domain::{
        ArchitectureZoom, Complexity, FileKind, ImpactFile, ImpactSort, ImplementationFuture,
        RepoFile, RepoModel, RiskLevel,
    };

    fn app() -> App {
        let dir = tempfile::tempdir().unwrap();
        let cli = Cli::parse_from(["brf", dir.path().to_str().unwrap()]);
        App::from_cli(cli, "test-key".to_string())
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
                change_needed: "enqueue parsing".to_string(),
            }],
            risk_summary: vec!["PII path".to_string()],
            tests_to_add: vec!["returns job id".to_string()],
            futures: vec![ImplementationFuture {
                name: "Queue Worker".to_string(),
                description: "Move parsing async".to_string(),
                complexity: Complexity::Medium,
                risk: RiskLevel::Medium,
                architecture: vec![],
                affected_files: vec!["workers/parser.ts".to_string()],
                benefits: vec!["faster upload".to_string()],
                drawbacks: vec!["more moving parts".to_string()],
                patch_plan: vec!["add queue".to_string()],
                test_plan: vec!["status test".to_string()],
            }],
            recommended_future: "Queue Worker".to_string(),
        }
    }

    fn sorted_analysis() -> ImpactAnalysis {
        let mut analysis = analysis();
        analysis.impact_path = vec![
            ImpactFile {
                path: "low.ts".to_string(),
                reason: "low".to_string(),
                impact_score: 3,
                confidence: 90,
                risk: RiskLevel::Low,
                change_needed: "small change".to_string(),
            },
            ImpactFile {
                path: "high.ts".to_string(),
                reason: "high".to_string(),
                impact_score: 10,
                confidence: 90,
                risk: RiskLevel::High,
                change_needed: "large change".to_string(),
            },
            ImpactFile {
                path: "mid.ts".to_string(),
                reason: "mid".to_string(),
                impact_score: 7,
                confidence: 90,
                risk: RiskLevel::Medium,
                change_needed: "medium change".to_string(),
            },
        ];
        analysis
    }

    fn repo_model() -> RepoModel {
        RepoModel {
            repo_name: "resume-interview".to_string(),
            root_path: "/tmp/resume-interview".to_string(),
            frameworks: vec!["Next.js".to_string()],
            files: vec![
                RepoFile {
                    path: "app/api/upload/route.ts".to_string(),
                    kind: FileKind::Route,
                    size: 1200,
                    symbols: vec!["POST".to_string()],
                    imports: vec!["services/parser".to_string()],
                    snippets: vec!["export async function POST() {}".to_string()],
                },
                RepoFile {
                    path: "workers/parser.ts".to_string(),
                    kind: FileKind::Worker,
                    size: 650,
                    symbols: vec!["parseJob".to_string()],
                    imports: vec![],
                    snippets: vec!["export async function parseJob() {}".to_string()],
                },
            ],
            routes: vec![],
            tests: vec![],
            config_files: vec![],
            risk_signals: vec![],
        }
    }

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    #[test]
    fn prepared_repo_path_drives_scan_and_default_output_dir() {
        let clone_dir = tempfile::tempdir().unwrap();
        let cli = Cli::parse_from(["brf", "https://github.com/acme/widget"]);
        let app = App::from_prepared_cli(
            cli,
            "test-key".to_string(),
            PreparedRepo {
                local_path: clone_dir.path().to_path_buf(),
                source_label: "github.com/acme/widget".to_string(),
                temporary_clone: true,
            },
        );

        assert_eq!(app.repo_path, clone_dir.path());
        assert_eq!(app.output_dir, clone_dir.path());
    }

    #[test]
    fn stream_progress_does_not_switch_scan_to_impact_effects() {
        let mut app = app();
        let (tx, _rx) = mpsc::unbounded_channel();
        app.screen = Screen::RepoScan;
        app.animation_stage = AnimationStage::ScanningSweep;

        app.handle_app_event(AppEvent::AnalysisProgress("{".to_string()), tx);

        assert_eq!(app.screen, Screen::RepoScan);
        assert_eq!(app.animation_stage, AnimationStage::StreamShimmer);
        assert_ne!(app.animation_stage, AnimationStage::ImpactTrace);
        assert_ne!(app.animation_stage, AnimationStage::ImpactToFutures);
    }

    #[test]
    fn impact_and_futures_navigation_use_explicit_transition_stages() {
        let mut app = app();
        let (tx, _rx) = mpsc::unbounded_channel();

        app.handle_app_event(AppEvent::AnalysisComplete(analysis()), tx.clone());
        assert_eq!(app.screen, Screen::ImpactExplorer);
        assert_eq!(app.animation_stage, AnimationStage::ImpactTrace);

        app.handle_explorer_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE), tx);
        assert_eq!(app.screen, Screen::FuturesCompare);
        assert_eq!(app.animation_stage, AnimationStage::ImpactToFutures);

        app.handle_futures_key(KeyEvent::new(KeyCode::BackTab, KeyModifiers::SHIFT));
        assert_eq!(app.screen, Screen::ImpactExplorer);
        assert_eq!(app.animation_stage, AnimationStage::FuturesToImpact);
    }

    #[test]
    fn impact_paths_default_to_high_to_low_sort() {
        let mut app = app();
        let (tx, _rx) = mpsc::unbounded_channel();

        app.handle_app_event(AppEvent::AnalysisComplete(sorted_analysis()), tx);

        assert_eq!(app.impact_sort, ImpactSort::HighToLow);
        assert_eq!(app.ordered_impact_indices(), vec![1, 2, 0]);
        assert_eq!(app.selected_impact_file().unwrap().path, "high.ts");
    }

    #[test]
    fn s_cycles_impact_sort_and_preserves_selected_file() {
        let mut app = app();
        let (tx, _rx) = mpsc::unbounded_channel();
        app.handle_app_event(AppEvent::AnalysisComplete(sorted_analysis()), tx.clone());

        app.handle_key(key(KeyCode::Char('s')), tx.clone());
        assert_eq!(app.impact_sort, ImpactSort::LowToHigh);
        assert_eq!(app.ordered_impact_indices(), vec![0, 2, 1]);
        assert_eq!(app.selected_impact_file().unwrap().path, "high.ts");

        app.handle_key(key(KeyCode::Char('s')), tx);
        assert_eq!(app.impact_sort, ImpactSort::ModelOrder);
        assert_eq!(app.ordered_impact_indices(), vec![0, 1, 2]);
        assert_eq!(app.selected_impact_file().unwrap().path, "high.ts");
    }

    #[test]
    fn tab_navigation_stays_between_impact_and_futures() {
        let mut app = app();
        let (tx, _rx) = mpsc::unbounded_channel();
        app.handle_app_event(AppEvent::AnalysisComplete(analysis()), tx.clone());
        app.handle_explorer_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE), tx);

        assert_eq!(app.screen, Screen::FuturesCompare);
    }

    #[test]
    fn transition_stage_returns_to_stable_state_after_animation_window() {
        let mut app = app();
        app.screen = Screen::FuturesCompare;
        app.restart_stage(AnimationStage::ImpactToFutures);

        for _ in 0..SCREEN_TRANSITION_RESET_TICKS {
            app.tick();
        }

        assert_eq!(app.animation_stage, AnimationStage::LockIn);
    }

    #[test]
    fn transition_stage_remains_active_before_requested_duration_finishes() {
        let mut app = app();
        app.screen = Screen::FuturesCompare;
        app.restart_stage(AnimationStage::ImpactToFutures);

        for _ in 0..5 {
            app.tick();
        }

        assert_eq!(app.animation_stage, AnimationStage::ImpactToFutures);
    }

    #[test]
    fn g_opens_architecture_from_analysis_screens_and_preserves_return_context() {
        for source_screen in [
            Screen::ImpactExplorer,
            Screen::FuturesCompare,
            Screen::FileDetail,
            Screen::RepoTree,
        ] {
            let mut app = app();
            let (tx, _rx) = mpsc::unbounded_channel();
            app.repo_model = Some(repo_model());
            app.impact_analysis = Some(analysis());
            app.screen = source_screen;

            assert!(!app.handle_key(key(KeyCode::Char('g')), tx));

            assert_eq!(app.screen, Screen::ArchitectureScaffold);
            app.handle_key(key(KeyCode::Esc), mpsc::unbounded_channel().0);
            assert_eq!(app.screen, source_screen);
        }
    }

    #[test]
    fn g_before_analysis_sets_status_and_stays_on_current_screen() {
        let mut app = app();
        let (tx, _rx) = mpsc::unbounded_channel();
        app.screen = Screen::ImpactExplorer;

        app.handle_key(key(KeyCode::Char('g')), tx);

        assert_eq!(app.screen, Screen::ImpactExplorer);
        assert!(app.status.contains("Run impact analysis"));
    }

    #[test]
    fn t_opens_repo_tree_from_analysis_screens_and_preserves_return_context() {
        for source_screen in [
            Screen::ImpactExplorer,
            Screen::FuturesCompare,
            Screen::FileDetail,
            Screen::ArchitectureScaffold,
        ] {
            let mut app = app();
            let (tx, _rx) = mpsc::unbounded_channel();
            app.repo_model = Some(repo_model());
            app.impact_analysis = Some(analysis());
            app.screen = source_screen;

            app.handle_key(key(KeyCode::Char('T')), tx);
            assert_eq!(app.screen, Screen::RepoTree);

            app.handle_key(key(KeyCode::Esc), mpsc::unbounded_channel().0);
            assert_eq!(app.screen, source_screen);
        }
    }

    #[test]
    fn t_before_repo_model_sets_status_and_stays_on_current_screen() {
        let mut app = app();
        let (tx, _rx) = mpsc::unbounded_channel();
        app.screen = Screen::ImpactExplorer;
        app.impact_analysis = Some(analysis());

        app.handle_key(key(KeyCode::Char('T')), tx);

        assert_eq!(app.screen, Screen::ImpactExplorer);
        assert!(app.status.contains("Scan repository"));
    }

    #[test]
    fn architecture_screen_supports_pan_and_zoom_keys() {
        let mut app = app();
        let (tx, _rx) = mpsc::unbounded_channel();
        app.repo_model = Some(repo_model());
        app.impact_analysis = Some(analysis());
        app.screen = Screen::ArchitectureScaffold;

        app.handle_key(key(KeyCode::Char('l')), tx.clone());
        assert_eq!(app.architecture_scroll_x, 8);
        app.handle_key(key(KeyCode::Char('j')), tx.clone());
        assert_eq!(app.architecture_scroll_y, 1);
        app.handle_key(key(KeyCode::Char('-')), tx.clone());
        assert_eq!(app.architecture_zoom, ArchitectureZoom::Compact);
        assert_eq!(app.architecture_scroll_x, 0);
        assert_eq!(app.architecture_scroll_y, 0);
        app.handle_key(key(KeyCode::Char('+')), tx.clone());
        assert_eq!(app.architecture_zoom, ArchitectureZoom::Normal);
        app.handle_key(key(KeyCode::Char('l')), tx.clone());
        app.handle_key(key(KeyCode::Char('j')), tx.clone());
        app.handle_key(key(KeyCode::Char('0')), tx);

        assert_eq!(app.architecture_zoom, ArchitectureZoom::Normal);
        assert_eq!(app.architecture_scroll_x, 0);
        assert_eq!(app.architecture_scroll_y, 0);
    }

    #[test]
    fn repo_tree_navigation_wraps_repo_files() {
        let mut app = app();
        let (tx, _rx) = mpsc::unbounded_channel();
        app.repo_model = Some(repo_model());
        app.screen = Screen::RepoTree;
        app.selected_repo_file_index = 0;

        app.handle_key(key(KeyCode::Up), tx.clone());
        assert_eq!(app.selected_repo_file_index, 1);

        app.handle_key(key(KeyCode::Down), tx);
        assert_eq!(app.selected_repo_file_index, 0);
    }

    #[test]
    fn help_returns_to_repo_tree_context() {
        let mut app = app();
        let (tx, _rx) = mpsc::unbounded_channel();
        app.repo_model = Some(repo_model());
        app.screen = Screen::RepoTree;

        app.handle_key(key(KeyCode::Char('?')), tx.clone());
        assert_eq!(app.screen, Screen::Help);

        app.handle_key(key(KeyCode::Esc), tx);
        assert_eq!(app.screen, Screen::RepoTree);
    }
}
