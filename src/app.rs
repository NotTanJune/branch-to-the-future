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
    domain::{AnimationStage, ImpactAnalysis, RepoModel, Screen},
    repo::scan_repo,
    ui,
};

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
    pub selected_future_index: usize,
    pub status: String,
    pub error: Option<String>,
    pub export_path: Option<PathBuf>,
    pub image_path: Option<PathBuf>,
    pub animation_stage: AnimationStage,
    pub trace_revealed: usize,
    pub stream_preview: String,
    pub cursor_visible: bool,
    pub spinner_frame: usize,
    pub effects: EffectManager<String>,
}

enum AppEvent {
    ScanComplete(RepoModel),
    AnalysisProgress(String),
    AnalysisComplete(ImpactAnalysis),
    ImageComplete(PathBuf),
    Error(String),
}

impl App {
    pub fn from_cli(cli: Cli, api_key: String) -> Self {
        let text_model = cli.resolved_model();
        let output_dir = cli.resolved_output_dir();
        let mut app = Self {
            repo_path: cli.repo_path,
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
            selected_future_index: 0,
            status: "Enter change request".to_string(),
            error: None,
            export_path: None,
            image_path: None,
            animation_stage: AnimationStage::BootReveal,
            trace_revealed: 0,
            stream_preview: String::new(),
            cursor_visible: true,
            spinner_frame: 0,
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
                self.screen = Screen::ImpactExplorer;
                self.set_stage(AnimationStage::ImpactTrace);
            }
            AppEvent::ImageComplete(path) => {
                self.image_path = Some(path.clone());
                self.status = format!("Architecture diagram generated {}", path.display());
                self.screen = Screen::ArtifactGeneration;
                self.restart_stage(AnimationStage::DiagramReveal);
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

        match self.screen {
            Screen::Input => self.handle_input_key(key, tx),
            Screen::RepoScan => {}
            Screen::ImpactExplorer => self.handle_explorer_key(key, tx),
            Screen::FileDetail => self.handle_detail_key(key),
            Screen::FuturesCompare => self.handle_futures_key(key),
            Screen::ArtifactGeneration => {
                if matches!(key.code, KeyCode::Char('e')) {
                    self.export();
                } else {
                    self.handle_modal_key(key);
                }
            }
            Screen::ExportSummary | Screen::Error | Screen::Help => self.handle_modal_key(key),
        }
        false
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

    fn handle_explorer_key(&mut self, key: KeyEvent, tx: mpsc::UnboundedSender<AppEvent>) {
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
            KeyCode::Char('g') => self.generate_image(tx),
            KeyCode::Char('e') => self.export(),
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
            self.image_path.as_deref(),
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

    fn generate_image(&mut self, tx: mpsc::UnboundedSender<AppEvent>) {
        let (Some(analysis), Some(repo)) = (&self.impact_analysis, &self.repo_model) else {
            self.status = "Run impact analysis before generating architecture diagram".to_string();
            return;
        };
        let client = OpenAiClient::new(
            self.api_key.clone(),
            self.text_model.clone(),
            self.reasoning_effort,
        )
        .with_limits(self.max_output_tokens, self.max_prompt_files);
        let analysis = analysis.clone();
        let change_request = self.change_request.clone();
        let repo_root = PathBuf::from(&repo.root_path);
        let tx_status = "Generating architecture diagram with OpenAI image tool".to_string();
        self.status = tx_status;
        let path = repo_root.join("branch-futures-architecture.png");
        self.image_path = Some(path.clone());
        self.screen = Screen::ArtifactGeneration;
        self.restart_stage(AnimationStage::DiagramReveal);
        tokio::spawn(async move {
            let event = match client
                .generate_architecture_diagram(&analysis, &change_request, &path)
                .await
            {
                Ok(()) => AppEvent::ImageComplete(path),
                Err(error) => AppEvent::Error(error.to_string()),
            };
            let _ = tx.send(event);
        });
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

#[cfg(test)]
mod tests {
    use super::*;

    use crate::domain::{Complexity, ImpactFile, ImplementationFuture, RiskLevel};

    fn app() -> App {
        let dir = tempfile::tempdir().unwrap();
        let cli = Cli::parse_from(["brf", dir.path().to_str().unwrap()]);
        App::from_cli(cli, "test-key".to_string())
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
                change_needed: "enqueue parsing".to_string(),
            }],
            risk_summary: vec!["PII path".to_string()],
            tests_to_add: vec!["returns job id".to_string()],
            futures: vec![ImplementationFuture {
                name: "Queue Worker".to_string(),
                description: "Move parsing async".to_string(),
                complexity: Complexity::Medium,
                risk: RiskLevel::Medium,
                affected_files: vec!["workers/parser.ts".to_string()],
                benefits: vec!["faster upload".to_string()],
                drawbacks: vec!["more moving parts".to_string()],
                patch_plan: vec!["add queue".to_string()],
                test_plan: vec!["status test".to_string()],
            }],
            recommended_future: "Queue Worker".to_string(),
        }
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
}
