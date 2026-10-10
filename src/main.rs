use std::{
    fs, io,
    path::PathBuf,
    sync::mpsc,
    time::{Duration, Instant},
};

use crossterm::{
    event::{
        self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent,
        MouseEventKind,
    },
    execute,
};
use mplib::{MicropubService, Post, PostStatus, publish_post};
use ratatui::{
    DefaultTerminal, Frame,
    layout::{Alignment, Constraint, Layout, Position, Rect},
    style::{Color, Modifier, Style},
    symbols::border,
    widgets::{Block, Clear, Padding, Paragraph},
};
use tui_textarea::{CursorRenderMode, TextArea, WrapMode};

fn main() -> io::Result<()> {
    let mut terminal = ratatui::init();
    let result = App::default().run(&mut terminal);
    ratatui::restore();
    result
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Focus {
    Textbox,
    HelpButton,
    ClearButton,
    SaveDraftButton,
    PostButton,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum QuitDialogFocus {
    Cancel,
    Discard,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum PublishTarget {
    Draft,
    Post,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum PublishDialogFocus {
    Cancel,
    Publish,
}

/// Outcome of a background publish: the new post's result on success, or a
/// message to display on failure.
type PublishOutcome = Result<mplib::PostResult, String>;

/// A publish request running on a worker thread.
struct PublishJob {
    target: PublishTarget,
    receiver: mpsc::Receiver<PublishOutcome>,
}

/// Frames for the "Publishing..." spinner, advanced once per event-loop tick.
const SPINNER_FRAMES: [&str; 4] = ["|", "/", "-", "\\"];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AlertKind {
    Published,
    Error,
}

#[derive(Clone, PartialEq, Eq)]
struct Alert {
    kind: AlertKind,
    message: String,
}

impl Alert {
    fn published(message: String) -> Self {
        Self {
            kind: AlertKind::Published,
            message,
        }
    }

    fn error(message: String) -> Self {
        Self {
            kind: AlertKind::Error,
            message,
        }
    }

    fn title(&self) -> &'static str {
        match self.kind {
            AlertKind::Published => " Published ",
            AlertKind::Error => " Error ",
        }
    }

    fn border_color(&self) -> Color {
        match self.kind {
            AlertKind::Published => Color::Cyan,
            AlertKind::Error => Color::Red,
        }
    }
}

struct App {
    textarea: TextArea<'static>,
    focus: Focus,
    hovered: Option<Focus>,
    should_quit: bool,
    quit_dialog: bool,
    quit_dialog_focus: QuitDialogFocus,
    help_sidebar: bool,
    help_textarea: TextArea<'static>,
    publish_dialog: Option<PublishTarget>,
    publish_dialog_focus: PublishDialogFocus,
    publish_job: Option<PublishJob>,
    spinner_frame: usize,
    alert: Option<Alert>,
    last_published: Option<String>,
    endpoint: EndpointState,
    service_api_url: Option<String>,
    service_auth_token: Option<String>,
    extract_title: bool,
    draft_path: Option<PathBuf>,
    last_saved: String,
    control_areas: ControlAreas,
    quit_dialog_areas: QuitDialogAreas,
    publish_dialog_areas: PublishDialogAreas,
    alert_button_area: Rect,
}

#[derive(Default)]
struct ControlAreas {
    textbox: Rect,
    help_button: Rect,
    clear_button: Rect,
    save_draft_button: Rect,
    post_button: Rect,
}

#[derive(Default)]
struct QuitDialogAreas {
    cancel_button: Rect,
    discard_button: Rect,
}

#[derive(Default)]
struct PublishDialogAreas {
    cancel_button: Rect,
    publish_button: Rect,
}

impl Default for App {
    fn default() -> Self {
        Self::new(load_micropub_settings(), default_draft_path())
    }
}

impl App {
    /// Build an app with explicit settings and draft path. A `None` draft
    /// path disables autosave, which keeps tests off the real filesystem.
    fn new(settings: MicropubSettings, draft_path: Option<PathBuf>) -> Self {
        let mut textarea = TextArea::default();
        textarea.set_wrap_mode(WrapMode::WordOrGlyph);
        let mut help_textarea = TextArea::new(vec![
            "Editor shortcuts".to_string(),
            "F1 / Help       Toggle shortcuts".to_string(),
            "F2 / Reload     Reload config".to_string(),
            "Tab             Next control".to_string(),
            "Shift+Tab       Previous control".to_string(),
            "Enter / Space   Activate control".to_string(),
            "Ctrl+Enter      Publish post".to_string(),
            "Alt+Ctrl+Enter  Publish draft".to_string(),
            "Ctrl+Z          Undo".to_string(),
            "Ctrl+Y          Redo".to_string(),
            "Ctrl+V          Paste".to_string(),
            "Esc / Ctrl+Q    Quit".to_string(),
            "Autosave        Draft autosaved locally".to_string(),
            "".to_string(),
            "Confirmation dialog shortcuts".to_string(),
            "C / Esc         Cancel".to_string(),
            "D / Q           Discard and quit".to_string(),
            "P               Publish".to_string(),
            "O               OK (publishing dialog)".to_string(),
        ]);
        help_textarea.set_cursor_render_mode(CursorRenderMode::Hidden);
        help_textarea.set_wrap_mode(WrapMode::WordOrGlyph);
        let confirmation_row = help_textarea
            .lines()
            .iter()
            .position(|line| line == "Confirmation dialog shortcuts")
            .expect("help text should contain the confirmation section header");
        let confirmation_len = help_textarea.lines()[confirmation_row].chars().count();
        help_textarea.custom_highlight(
            ((confirmation_row, 0), (confirmation_row, confirmation_len)),
            Style::default().add_modifier(Modifier::UNDERLINED),
            0,
        );

        Self {
            textarea,
            focus: Focus::Textbox,
            hovered: None,
            should_quit: false,
            quit_dialog: false,
            quit_dialog_focus: QuitDialogFocus::Cancel,
            help_sidebar: false,
            help_textarea,
            publish_dialog: None,
            publish_dialog_focus: PublishDialogFocus::Cancel,
            publish_job: None,
            spinner_frame: 0,
            alert: None,
            last_published: None,
            endpoint: settings.endpoint,
            service_api_url: settings.service_api_url,
            service_auth_token: settings.service_auth_token,
            extract_title: settings.extract_title,
            draft_path,
            last_saved: String::new(),
            control_areas: ControlAreas::default(),
            quit_dialog_areas: QuitDialogAreas::default(),
            publish_dialog_areas: PublishDialogAreas::default(),
            alert_button_area: Rect::default(),
        }
    }
}

/// Micropub settings loaded from ~/.config/mp/config.toml.
#[derive(Default)]
struct MicropubSettings {
    /// Endpoint configuration state, driving the header display.
    endpoint: EndpointState,
    /// Full API URL used to publish; present when api_url is configured and non-empty.
    service_api_url: Option<String>,
    /// Auth token used to publish; present when auth_token is configured and non-empty.
    service_auth_token: Option<String>,
    /// Whether to extract a title from a leading markdown header, per [default_behavior].
    extract_title: bool,
}

/// What the header endpoint display knows about the Micropub configuration.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
enum EndpointState {
    /// Fully configured; carries the API URL with protocol prefix stripped.
    Configured(String),
    /// api_url is set but auth_token is missing or empty; carries the stripped URL.
    MissingToken(String),
    /// No api_url configured.
    #[default]
    Missing,
    /// The config file could not be read; carries the OS error message.
    ReadError(String),
    /// The config file could not be parsed as TOML; carries the parse error.
    ParseError(String),
}

/// Read the Micropub settings from ~/.config/mp/config.toml. A missing file
/// is normal for first-time users and yields default (unconfigured) settings.
fn load_micropub_settings() -> MicropubSettings {
    let Some(config_path) = std::env::home_dir().map(|home| home.join(".config/mp/config.toml"))
    else {
        return MicropubSettings::default();
    };
    match fs::read_to_string(config_path) {
        Ok(config_content) => parse_micropub_settings(&config_content),
        Err(e) if e.kind() == io::ErrorKind::NotFound => MicropubSettings::default(),
        Err(e) => MicropubSettings {
            endpoint: EndpointState::ReadError(e.to_string()),
            ..Default::default()
        },
    }
}

/// Resolve the draft autosave path: `$XDG_STATE_HOME/mptui/draft.md`,
/// falling back to `~/.local/state/mptui/draft.md`. Returns `None` when no
/// usable base directory exists, which disables autosave.
fn default_draft_path() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::home_dir().map(|home| home.join(".local/state")))?;
    Some(base.join("mptui/draft.md"))
}

/// Parse Micropub settings from the contents of an mp config file.
///
/// Kept free of file access so it can be unit tested without touching the
/// user's real configuration.
fn parse_micropub_settings(config_content: &str) -> MicropubSettings {
    let mut settings = MicropubSettings::default();

    let config: toml::Table = match config_content.parse() {
        Ok(config) => config,
        Err(e) => {
            let first_line = e
                .to_string()
                .lines()
                .next()
                .unwrap_or("invalid TOML")
                .to_string();
            settings.endpoint = EndpointState::ParseError(first_line);
            return settings;
        }
    };

    let read_service_value = |key: &str| {
        config
            .get("service")?
            .get(key)?
            .as_str()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
    };
    let api_url = read_service_value("api_url");
    let auth_token = read_service_value("auth_token");

    if let Some(api_url) = api_url {
        let display = strip_url_protocol(&api_url).to_string();
        settings.service_api_url = Some(api_url);
        settings.endpoint = if auth_token.is_some() {
            EndpointState::Configured(display)
        } else {
            EndpointState::MissingToken(display)
        };
    }
    settings.service_auth_token = auth_token;

    settings.extract_title = config
        .get("default_behavior")
        .and_then(|section| section.get("extract_title"))
        .and_then(|value| value.as_bool())
        .unwrap_or(false);

    settings
}

/// Remove the leading `https://` or `http://` protocol prefix from a URL.
fn strip_url_protocol(url: &str) -> &str {
    url.strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
        .unwrap_or(url)
}

/// Shorten text to at most `max_chars` characters, appending "..." when cut.
fn truncate_end(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        text.to_string()
    } else {
        format!("{}...", text.chars().take(max_chars).collect::<String>())
    }
}

/// Heuristic check for authentication failures in a publish error message.
/// mplib reports them as e.g. "API error: unauthorized - ...".
fn is_auth_failure(message: &str) -> bool {
    let message = message.to_lowercase();
    ["unauthorized", "invalid_token", "forbidden", "401", "403"]
        .iter()
        .any(|hint| message.contains(hint))
}

/// Compute the centered dialog area used by the modal dialogs.
fn centered_dialog_area(area: Rect) -> Rect {
    let dialog_area = Layout::vertical([
        Constraint::Percentage(30),
        Constraint::Percentage(40),
        Constraint::Percentage(30),
    ])
    .split(area)[1];
    Layout::horizontal([
        Constraint::Percentage(20),
        Constraint::Percentage(60),
        Constraint::Percentage(20),
    ])
    .split(dialog_area)[1]
}

/// Publish a post to completion: rebuild the service, run the async mplib
/// call on a fresh single-threaded runtime, and return the outcome.
/// Intended as the body of the background publish worker thread.
fn run_publish(api_url: String, auth_token: String, post: Post) -> PublishOutcome {
    let service = MicropubService::from_args(api_url, auth_token)
        .map_err(|e| format!("invalid Micropub configuration: {e}"))?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| format!("failed to start publish runtime: {e}"))?;
    runtime
        .block_on(publish_post(post, &service))
        .map_err(|e| e.to_string())
}

impl App {
    fn run(mut self, terminal: &mut DefaultTerminal) -> io::Result<()> {
        execute!(io::stdout(), event::EnableMouseCapture)?;
        self.restore_draft();

        let result = self.run_loop(terminal);
        let disable_result = execute!(io::stdout(), event::DisableMouseCapture);

        result.and(disable_result)
    }

    fn run_loop(&mut self, terminal: &mut DefaultTerminal) -> io::Result<()> {
        // Autosave at most ~once a second, and only when the text changed.
        let mut last_autosave = Instant::now();
        while !self.should_quit {
            terminal.draw(|frame| self.draw(frame))?;
            self.handle_events()?;
            if self.draft_path.is_some()
                && self.draft_text() != self.last_saved
                && last_autosave.elapsed() >= Duration::from_millis(750)
            {
                self.autosave_draft();
                last_autosave = Instant::now();
            }
        }
        // Final save on the way out so nothing is lost between ticks.
        // This is a no-op after publish, clear, or deliberate discard.
        self.autosave_draft();

        Ok(())
    }

    fn draw(&mut self, frame: &mut Frame) {
        let areas = Layout::vertical([
            Constraint::Length(3),
            Constraint::Min(5),
            Constraint::Length(1),
        ])
        .split(frame.area());

        let header_areas = Layout::horizontal([
            Constraint::Length(8),
            Constraint::Min(10),
            Constraint::Length(8),
            Constraint::Length(9),
            Constraint::Length(17),
            Constraint::Length(16),
        ])
        .spacing(1)
        .split(areas[0]);
        let can_clear = !self.textarea.is_empty();
        let can_publish = self.has_publishable_text();
        let label = Paragraph::new("MPTUI")
            .style(
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )
            .alignment(Alignment::Left)
            .block(Block::default().padding(Padding::new(1, 1, 1, 1)));
        frame.render_widget(label, header_areas[0]);
        let (endpoint_text, endpoint_style): (String, Style) = match &self.endpoint {
            EndpointState::Configured(api_url) => {
                (api_url.clone(), Style::default().fg(Color::DarkGray))
            }
            EndpointState::MissingToken(api_url) => (
                format!("{api_url} (no token)"),
                Style::default().fg(Color::Yellow),
            ),
            EndpointState::Missing => (
                "No MicroPub Endpoint Configured!".to_string(),
                Style::default().fg(Color::Red),
            ),
            EndpointState::ReadError(message) | EndpointState::ParseError(message) => (
                format!("Config error: {message}"),
                Style::default().fg(Color::Red),
            ),
        };
        let endpoint = Paragraph::new(endpoint_text)
            .style(endpoint_style)
            .alignment(Alignment::Left)
            .block(Block::default().padding(Padding::new(1, 1, 1, 1)));
        frame.render_widget(endpoint, header_areas[1]);
        self.render_button(frame, header_areas[2], "Help", Focus::HelpButton);
        self.render_button_with_enabled(
            frame,
            header_areas[3],
            "Clear",
            Focus::ClearButton,
            can_clear,
        );
        self.render_button_with_enabled(
            frame,
            header_areas[4],
            "Publish Draft",
            Focus::SaveDraftButton,
            can_publish,
        );
        self.render_button_with_enabled(
            frame,
            header_areas[5],
            "Publish Post",
            Focus::PostButton,
            can_publish,
        );

        let textbox_style = if self.focus == Focus::Textbox {
            Style::default().fg(Color::Cyan)
        } else {
            Style::default().fg(Color::DarkGray)
        };
        let editor_area = if self.help_sidebar {
            let columns =
                Layout::horizontal([Constraint::Percentage(70), Constraint::Percentage(30)])
                    .spacing(1)
                    .split(areas[1]);
            self.draw_help_sidebar(frame, columns[1]);
            columns[0]
        } else {
            areas[1]
        };

        self.textarea.set_cursor_line_style(Style::default());
        self.textarea.set_block(
            Block::bordered()
                .border_set(border::ROUNDED)
                .title(" Blog Post ")
                .padding(Padding::horizontal(1))
                .border_style(textbox_style),
        );
        frame.render_widget(&self.textarea, editor_area);

        self.control_areas = ControlAreas {
            textbox: editor_area,
            help_button: header_areas[2],
            clear_button: header_areas[3],
            save_draft_button: header_areas[4],
            post_button: header_areas[5],
        };

        let commands = Paragraph::new(
            "F1 Help  F2 Reload  Ctrl+Enter Publish Post  Alt+Ctrl+Enter Publish Draft  Esc Quit",
        )
        .style(Style::default().fg(Color::DarkGray))
        .alignment(Alignment::Center);
        let status = self.status_text();
        let status_width = ratatui::text::Line::from(status.as_str()).width() as u16;
        let footer = Layout::horizontal([Constraint::Min(10), Constraint::Length(status_width)])
            .split(areas[2]);
        frame.render_widget(commands, footer[0]);
        frame.render_widget(
            Paragraph::new(status)
                .style(Style::default().fg(Color::DarkGray))
                .alignment(Alignment::Right),
            footer[1],
        );

        if self.quit_dialog {
            self.draw_quit_dialog(frame);
        } else if self.publish_dialog.is_some() {
            self.draw_publish_dialog(frame);
        } else if self.publish_job.is_some() {
            self.draw_publishing_dialog(frame);
        } else if self.alert.is_some() {
            self.draw_alert_dialog(frame);
        }
    }

    fn draw_help_sidebar(&mut self, frame: &mut Frame, area: Rect) {
        self.help_textarea
            .set_cursor_render_mode(CursorRenderMode::Hidden);
        self.help_textarea.set_block(
            Block::bordered()
                .border_set(border::ROUNDED)
                .title(" Keyboard Shortcuts ")
                .padding(Padding::horizontal(1))
                .border_style(Style::default().fg(Color::DarkGray)),
        );
        frame.render_widget(&self.help_textarea, area);
    }

    fn draw_publish_dialog(&mut self, frame: &mut Frame) {
        let target = self
            .publish_dialog
            .expect("publish dialog should have a target while rendering");
        let title = match target {
            PublishTarget::Draft => " Publish Draft ",
            PublishTarget::Post => " Publish Post ",
        };
        let message = match target {
            PublishTarget::Draft => "Publish this draft?",
            PublishTarget::Post => "Publish this post?",
        };
        let dialog_area = Layout::vertical([
            Constraint::Percentage(30),
            Constraint::Percentage(40),
            Constraint::Percentage(30),
        ])
        .split(frame.area())[1];
        let dialog_area = Layout::horizontal([
            Constraint::Percentage(20),
            Constraint::Percentage(60),
            Constraint::Percentage(20),
        ])
        .split(dialog_area)[1];

        let dialog = Block::bordered()
            .title(title)
            .border_style(Style::default().fg(Color::Cyan));
        let content_area = dialog.inner(dialog_area);
        frame.render_widget(Clear, dialog_area);
        frame.render_widget(dialog, dialog_area);

        let content = Layout::vertical([
            Constraint::Min(1),
            Constraint::Length(2),
            Constraint::Length(3),
            Constraint::Min(1),
        ])
        .split(content_area);
        frame.render_widget(
            Paragraph::new(message).alignment(Alignment::Center),
            content[1],
        );

        let button_areas =
            Layout::horizontal([Constraint::Percentage(40), Constraint::Percentage(60)])
                .spacing(1)
                .split(content[2]);
        self.publish_dialog_areas = PublishDialogAreas {
            cancel_button: button_areas[0],
            publish_button: button_areas[1],
        };
        self.render_publish_dialog_button(
            frame,
            button_areas[0],
            "Cancel",
            PublishDialogFocus::Cancel,
        );
        self.render_publish_dialog_button(
            frame,
            button_areas[1],
            "Publish",
            PublishDialogFocus::Publish,
        );
    }

    fn draw_publishing_dialog(&mut self, frame: &mut Frame) {
        let target = self
            .publish_job
            .as_ref()
            .map(|job| job.target)
            .expect("publishing dialog should have a job while rendering");
        let title = match target {
            PublishTarget::Draft => " Publishing Draft ",
            PublishTarget::Post => " Publishing Post ",
        };
        let noun = match target {
            PublishTarget::Draft => "draft",
            PublishTarget::Post => "post",
        };
        let spinner = SPINNER_FRAMES[self.spinner_frame % SPINNER_FRAMES.len()];
        let message = format!("Publishing {noun} {spinner}");
        let dialog_area = centered_dialog_area(frame.area());

        let dialog = Block::bordered()
            .title(title)
            .border_style(Style::default().fg(Color::Cyan));
        let content_area = dialog.inner(dialog_area);
        frame.render_widget(Clear, dialog_area);
        frame.render_widget(dialog, dialog_area);

        frame.render_widget(
            Paragraph::new(message).alignment(Alignment::Center),
            content_area,
        );
    }

    fn draw_alert_dialog(&mut self, frame: &mut Frame) {
        let alert = self
            .alert
            .clone()
            .expect("alert dialog should have a message while rendering");
        let dialog_area = centered_dialog_area(frame.area());

        let dialog = Block::bordered()
            .title(alert.title())
            .border_style(Style::default().fg(alert.border_color()));
        let content_area = dialog.inner(dialog_area);
        frame.render_widget(Clear, dialog_area);
        frame.render_widget(dialog, dialog_area);

        let content = Layout::vertical([
            Constraint::Min(1),
            Constraint::Length(alert.message.lines().count() as u16),
            Constraint::Min(1),
            Constraint::Length(3),
            Constraint::Min(1),
        ])
        .split(content_area);
        frame.render_widget(
            Paragraph::new(alert.message).alignment(Alignment::Center),
            content[1],
        );

        let button_area = Layout::horizontal([Constraint::Length(10)])
            .flex(ratatui::layout::Flex::Center)
            .split(content[3])[0];
        self.alert_button_area = button_area;
        let style = Style::default()
            .fg(Color::Black)
            .bg(Color::Cyan)
            .add_modifier(Modifier::BOLD);
        let button = Paragraph::new("OK")
            .alignment(Alignment::Center)
            .style(style)
            .block(
                Block::bordered()
                    .border_set(border::ROUNDED)
                    .border_style(style),
            );
        frame.render_widget(button, button_area);
    }

    fn draw_quit_dialog(&mut self, frame: &mut Frame) {
        let dialog_area = Layout::vertical([
            Constraint::Percentage(30),
            Constraint::Percentage(40),
            Constraint::Percentage(30),
        ])
        .split(frame.area())[1];
        let dialog_area = Layout::horizontal([
            Constraint::Percentage(20),
            Constraint::Percentage(60),
            Constraint::Percentage(20),
        ])
        .split(dialog_area)[1];

        let dialog = Block::bordered()
            .title(" Quit ")
            .border_style(Style::default().fg(Color::Yellow));
        let content_area = dialog.inner(dialog_area);
        frame.render_widget(Clear, dialog_area);
        frame.render_widget(dialog, dialog_area);

        let content = Layout::vertical([
            Constraint::Min(1),
            Constraint::Length(2),
            Constraint::Length(3),
            Constraint::Min(1),
        ])
        .split(content_area);
        let message =
            Paragraph::new("Discard your current draft and quit?").alignment(Alignment::Center);
        frame.render_widget(message, content[1]);

        let button_areas =
            Layout::horizontal([Constraint::Percentage(40), Constraint::Percentage(60)])
                .spacing(1)
                .split(content[2]);
        self.quit_dialog_areas = QuitDialogAreas {
            cancel_button: button_areas[0],
            discard_button: button_areas[1],
        };
        self.render_quit_button(frame, button_areas[0], "Cancel", QuitDialogFocus::Cancel);
        self.render_quit_button(
            frame,
            button_areas[1],
            "Discard and Quit",
            QuitDialogFocus::Discard,
        );
    }

    fn render_quit_button(
        &self,
        frame: &mut Frame,
        area: Rect,
        label: &str,
        focus: QuitDialogFocus,
    ) {
        let style = if self.quit_dialog_focus == focus {
            Style::default()
                .fg(Color::Black)
                .bg(Color::Yellow)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::White)
        };
        let button = Paragraph::new(label)
            .alignment(Alignment::Center)
            .style(style)
            .block(
                Block::bordered()
                    .border_set(border::ROUNDED)
                    .border_style(style),
            );
        frame.render_widget(button, area);
    }

    fn render_button(
        &self,
        frame: &mut Frame,
        area: ratatui::layout::Rect,
        label: &str,
        focus: Focus,
    ) {
        self.render_button_with_enabled(frame, area, label, focus, true);
    }

    fn render_button_with_enabled(
        &self,
        frame: &mut Frame,
        area: Rect,
        label: &str,
        focus: Focus,
        enabled: bool,
    ) {
        let style = if !enabled {
            Style::default().fg(Color::DarkGray)
        } else if self.focus == focus || self.hovered == Some(focus) {
            Style::default()
                .fg(Color::Black)
                .bg(Color::Cyan)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::DarkGray)
        };
        let button = Paragraph::new(label)
            .alignment(Alignment::Center)
            .style(style)
            .block(
                Block::bordered()
                    .border_set(border::ROUNDED)
                    .border_style(style),
            );
        frame.render_widget(button, area);
    }

    fn render_publish_dialog_button(
        &self,
        frame: &mut Frame,
        area: Rect,
        label: &str,
        focus: PublishDialogFocus,
    ) {
        let style = if self.publish_dialog_focus == focus {
            Style::default()
                .fg(Color::Black)
                .bg(Color::Cyan)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::DarkGray)
        };
        let button = Paragraph::new(label)
            .alignment(Alignment::Center)
            .style(style)
            .block(
                Block::bordered()
                    .border_set(border::ROUNDED)
                    .border_style(style),
            );
        frame.render_widget(button, area);
    }

    fn handle_events(&mut self) -> io::Result<()> {
        if self.publish_job.is_some() {
            self.poll_publish();
        }

        // Tick faster while publishing so the spinner animates smoothly.
        let poll_timeout = if self.publish_job.is_some() { 100 } else { 250 };
        if event::poll(Duration::from_millis(poll_timeout))? {
            match event::read()? {
                Event::Key(key_event) if key_event.kind == KeyEventKind::Press => {
                    self.handle_key_event(key_event);
                }
                Event::Paste(text) if self.focus == Focus::Textbox => {
                    self.textarea.insert_str(text);
                }
                Event::Mouse(mouse_event) => self.handle_mouse_event(mouse_event),
                _ => {}
            }
        }

        Ok(())
    }

    fn handle_mouse_event(&mut self, mouse_event: MouseEvent) {
        if self.publish_job.is_some() {
            return;
        }
        if self.alert.is_some() {
            self.handle_alert_mouse_event(mouse_event);
            return;
        }
        if self.quit_dialog {
            self.handle_quit_dialog_mouse_event(mouse_event);
            return;
        }
        if self.publish_dialog.is_some() {
            self.handle_publish_dialog_mouse_event(mouse_event);
            return;
        }

        let position = Position::new(mouse_event.column, mouse_event.row);
        let can_clear = !self.textarea.is_empty();
        let can_publish = self.has_publishable_text();
        self.hovered = if self.control_areas.help_button.contains(position) {
            Some(Focus::HelpButton)
        } else if can_clear && self.control_areas.clear_button.contains(position) {
            Some(Focus::ClearButton)
        } else if can_publish && self.control_areas.save_draft_button.contains(position) {
            Some(Focus::SaveDraftButton)
        } else if can_publish && self.control_areas.post_button.contains(position) {
            Some(Focus::PostButton)
        } else {
            None
        };

        if self.control_areas.textbox.contains(position) {
            self.focus = Focus::Textbox;
            self.textarea.input(mouse_event);
            return;
        }

        if mouse_event.kind != MouseEventKind::Down(MouseButton::Left) {
            return;
        } else if self.control_areas.help_button.contains(position) {
            self.focus = Focus::HelpButton;
            self.help_sidebar = !self.help_sidebar;
        } else if can_clear && self.control_areas.clear_button.contains(position) {
            self.clear_input();
        } else if can_publish && self.control_areas.save_draft_button.contains(position) {
            self.focus = Focus::SaveDraftButton;
            self.open_publish_dialog(PublishTarget::Draft);
        } else if can_publish && self.control_areas.post_button.contains(position) {
            self.focus = Focus::PostButton;
            self.open_publish_dialog(PublishTarget::Post);
        }
    }

    fn handle_quit_dialog_mouse_event(&mut self, mouse_event: MouseEvent) {
        let position = Position::new(mouse_event.column, mouse_event.row);
        if self.quit_dialog_areas.cancel_button.contains(position) {
            self.quit_dialog_focus = QuitDialogFocus::Cancel;
        } else if self.quit_dialog_areas.discard_button.contains(position) {
            self.quit_dialog_focus = QuitDialogFocus::Discard;
        } else {
            return;
        }

        if mouse_event.kind == MouseEventKind::Down(MouseButton::Left) {
            self.activate_quit_dialog_focus();
        }
    }

    fn handle_publish_dialog_mouse_event(&mut self, mouse_event: MouseEvent) {
        let position = Position::new(mouse_event.column, mouse_event.row);
        if self.publish_dialog_areas.cancel_button.contains(position) {
            self.publish_dialog_focus = PublishDialogFocus::Cancel;
        } else if self.publish_dialog_areas.publish_button.contains(position) {
            self.publish_dialog_focus = PublishDialogFocus::Publish;
        } else {
            return;
        }

        if mouse_event.kind == MouseEventKind::Down(MouseButton::Left) {
            self.activate_publish_dialog_focus();
        }
    }

    fn handle_key_event(&mut self, key_event: KeyEvent) {
        if self.publish_job.is_some() {
            return;
        }
        if self.alert.is_some() {
            self.handle_alert_key_event(key_event);
            return;
        }
        if self.quit_dialog {
            self.handle_quit_dialog_key_event(key_event);
            return;
        }
        if self.publish_dialog.is_some() {
            self.handle_publish_dialog_key_event(key_event);
            return;
        }

        if key_event.code == KeyCode::Esc
            || (key_event.code == KeyCode::Char('q')
                && key_event.modifiers.contains(KeyModifiers::CONTROL))
        {
            self.request_quit();
            return;
        }

        if key_event.code == KeyCode::F(1) {
            self.help_sidebar = !self.help_sidebar;
            return;
        }

        if key_event.code == KeyCode::F(2) {
            self.reload_settings();
            return;
        }

        if self.focus == Focus::Textbox
            && key_event.code == KeyCode::Enter
            && key_event.modifiers.contains(KeyModifiers::ALT)
            && key_event.modifiers.contains(KeyModifiers::CONTROL)
            && self.has_publishable_text()
        {
            self.open_publish_dialog(PublishTarget::Draft);
            return;
        }

        if self.focus == Focus::Textbox
            && key_event.code == KeyCode::Enter
            && key_event.modifiers.contains(KeyModifiers::CONTROL)
            && self.has_publishable_text()
        {
            self.open_publish_dialog(PublishTarget::Post);
            return;
        }

        if self.focus == Focus::Textbox && key_event.modifiers.contains(KeyModifiers::CONTROL) {
            match key_event.code {
                KeyCode::Char('z') => {
                    self.textarea.undo();
                    return;
                }
                KeyCode::Char('y') => {
                    self.textarea.redo();
                    return;
                }
                KeyCode::Char('v') => {
                    self.textarea.paste();
                    return;
                }
                _ => {}
            }
        }

        match key_event.code {
            KeyCode::Tab => self.focus_next(),
            KeyCode::BackTab => self.focus_previous(),
            KeyCode::Enter | KeyCode::Char(' ')
                if self.focus == Focus::ClearButton && !self.textarea.is_empty() =>
            {
                self.clear_input()
            }
            KeyCode::Enter | KeyCode::Char(' ')
                if self.focus == Focus::SaveDraftButton && self.has_publishable_text() =>
            {
                self.open_publish_dialog(PublishTarget::Draft);
            }
            KeyCode::Enter | KeyCode::Char(' ')
                if self.focus == Focus::PostButton && self.has_publishable_text() =>
            {
                self.open_publish_dialog(PublishTarget::Post);
            }
            KeyCode::Enter | KeyCode::Char(' ') if self.focus == Focus::HelpButton => {
                self.help_sidebar = !self.help_sidebar;
            }
            _ if self.focus == Focus::Textbox => {
                self.textarea.input(key_event);
            }
            _ => {}
        }
    }

    fn handle_quit_dialog_key_event(&mut self, key_event: KeyEvent) {
        match key_event.code {
            KeyCode::Esc | KeyCode::Char('c' | 'C') => {
                self.quit_dialog = false;
            }
            KeyCode::Char('d' | 'D') | KeyCode::Char('q' | 'Q') => {
                self.quit_dialog_focus = QuitDialogFocus::Discard;
                self.activate_quit_dialog_focus();
            }
            KeyCode::Tab => {
                self.quit_dialog_focus = match self.quit_dialog_focus {
                    QuitDialogFocus::Cancel => QuitDialogFocus::Discard,
                    QuitDialogFocus::Discard => QuitDialogFocus::Cancel,
                };
            }
            KeyCode::BackTab => {
                self.quit_dialog_focus = match self.quit_dialog_focus {
                    QuitDialogFocus::Cancel => QuitDialogFocus::Discard,
                    QuitDialogFocus::Discard => QuitDialogFocus::Cancel,
                };
            }
            KeyCode::Enter => self.activate_quit_dialog_focus(),
            _ => {}
        }
    }

    fn handle_publish_dialog_key_event(&mut self, key_event: KeyEvent) {
        match key_event.code {
            KeyCode::Esc | KeyCode::Char('c' | 'C') => {
                self.publish_dialog = None;
            }
            KeyCode::Char('p' | 'P') => {
                self.publish_dialog_focus = PublishDialogFocus::Publish;
                self.activate_publish_dialog_focus();
            }
            KeyCode::Tab | KeyCode::BackTab => {
                self.publish_dialog_focus = match self.publish_dialog_focus {
                    PublishDialogFocus::Cancel => PublishDialogFocus::Publish,
                    PublishDialogFocus::Publish => PublishDialogFocus::Cancel,
                };
            }
            KeyCode::Enter => self.activate_publish_dialog_focus(),
            _ => {}
        }
    }

    fn activate_quit_dialog_focus(&mut self) {
        match self.quit_dialog_focus {
            QuitDialogFocus::Cancel => self.quit_dialog = false,
            QuitDialogFocus::Discard => {
                self.quit_dialog = false;
                self.delete_autosave();
                self.should_quit = true;
            }
        }
    }

    fn activate_publish_dialog_focus(&mut self) {
        match self.publish_dialog_focus {
            PublishDialogFocus::Cancel => self.publish_dialog = None,
            PublishDialogFocus::Publish => {
                let target = self.publish_dialog;
                self.publish_dialog = None;
                if let Some(target) = target {
                    self.start_publish(target);
                }
                self.focus = Focus::Textbox;
            }
        }
    }

    fn handle_alert_key_event(&mut self, key_event: KeyEvent) {
        match key_event.code {
            KeyCode::Esc | KeyCode::Enter | KeyCode::Char(' ') | KeyCode::Char('o' | 'O') => {
                self.alert = None;
            }
            _ => {}
        }
    }

    fn handle_alert_mouse_event(&mut self, mouse_event: MouseEvent) {
        let position = Position::new(mouse_event.column, mouse_event.row);
        if mouse_event.kind == MouseEventKind::Down(MouseButton::Left)
            && self.alert_button_area.contains(position)
        {
            self.alert = None;
        }
    }

    /// Start publishing the textarea content on a worker thread. The editor
    /// stays responsive while the request is in flight; the outcome is
    /// collected by `poll_publish`. Shows an error alert immediately when no
    /// endpoint is configured.
    fn start_publish(&mut self, target: PublishTarget) {
        if self.publish_job.is_some() {
            return;
        }

        let Some(api_url) = self.service_api_url.clone() else {
            self.alert = Some(Alert::error(
                "No Micropub endpoint configured. Cannot publish.".to_string(),
            ));
            return;
        };
        let Some(auth_token) = self.service_auth_token.clone() else {
            self.alert = Some(Alert::error(
                "No Micropub auth token configured. Cannot publish.".to_string(),
            ));
            return;
        };

        let body = self.textarea.lines().join("\n");
        let status = match target {
            PublishTarget::Draft => PostStatus::Draft,
            PublishTarget::Post => PostStatus::Published,
        };
        let post = if self.extract_title {
            Post::from_body_with_title_extraction(body, status)
        } else {
            Post::from_body(body, status)
        };

        let (sender, receiver) = mpsc::channel();
        std::thread::spawn(move || {
            let outcome = run_publish(api_url, auth_token, post);
            let _ = sender.send(outcome);
        });
        self.publish_job = Some(PublishJob { target, receiver });
    }

    /// Check a running publish job without blocking and advance the spinner.
    /// On completion the textarea is cleared and an alert confirms the
    /// publish; on failure the content is kept and the alert reports the error.
    fn poll_publish(&mut self) {
        let Some((target, received)) = self
            .publish_job
            .as_ref()
            .map(|job| (job.target, job.receiver.try_recv()))
        else {
            return;
        };
        self.spinner_frame = self.spinner_frame.wrapping_add(1);

        let noun = match target {
            PublishTarget::Draft => "Draft",
            PublishTarget::Post => "Post",
        };
        match received {
            Ok(Ok(result)) => {
                self.publish_job = None;
                self.clear_input();
                self.last_published = Some(result.url.clone());
                self.alert = Some(Alert::published(format!(
                    "{noun} published successfully.\nURL: {}",
                    result.url
                )));
            }
            Ok(Err(message)) => {
                self.publish_job = None;
                // The token may have been rotated outside the app; pick up
                // the current config so the next attempt uses fresh values.
                if is_auth_failure(&message) {
                    self.reload_settings();
                }
                self.alert = Some(Alert::error(format!("Failed to publish {noun}: {message}")));
            }
            Err(mpsc::TryRecvError::Empty) => {}
            Err(mpsc::TryRecvError::Disconnected) => {
                self.publish_job = None;
                self.alert = Some(Alert::error("Publish ended unexpectedly.".to_string()));
            }
        }
    }

    fn request_quit(&mut self) {
        if self.textarea.is_empty() {
            self.should_quit = true;
        } else {
            self.quit_dialog = true;
            self.quit_dialog_focus = QuitDialogFocus::Cancel;
        }
    }

    fn open_publish_dialog(&mut self, target: PublishTarget) {
        if self.publish_job.is_some() {
            return;
        }
        if self.has_publishable_text() {
            self.publish_dialog = Some(target);
            self.publish_dialog_focus = PublishDialogFocus::Cancel;
        }
    }

    /// Apply freshly loaded settings to the running app.
    fn apply_settings(&mut self, settings: MicropubSettings) {
        self.endpoint = settings.endpoint;
        self.service_api_url = settings.service_api_url;
        self.service_auth_token = settings.service_auth_token;
        self.extract_title = settings.extract_title;
    }

    /// Re-read the config file, e.g. after the user edits it externally.
    fn reload_settings(&mut self) {
        self.apply_settings(load_micropub_settings());
    }

    fn has_publishable_text(&self) -> bool {
        self.textarea
            .lines()
            .iter()
            .any(|line| !line.trim().is_empty())
    }

    fn clear_input(&mut self) {
        self.textarea.select_all();
        self.textarea.cut();
        self.delete_autosave();
        self.focus = Focus::Textbox;
    }

    /// Draft text currently in the editor.
    fn draft_text(&self) -> String {
        self.textarea.lines().join("\n")
    }

    /// Right-hand footer text: draft stats, cursor position, and publish state.
    fn status_text(&self) -> String {
        if self.publish_job.is_some() {
            return "Publishing...".to_string();
        }
        let text = self.draft_text();
        let words = text.split_whitespace().count();
        let chars = text.chars().count();
        let (row, col) = self.textarea.cursor();
        let mut status = format!(
            "Words: {words}  Chars: {chars}  Ln:{},Col:{}",
            row + 1,
            col + 1
        );
        if let Some(url) = &self.last_published {
            status.push_str(&format!("  Last: {}", truncate_end(url, 32)));
        }
        status
    }

    /// Restore a previously autosaved draft, if any. Runs once at startup.
    fn restore_draft(&mut self) {
        let Some(path) = self.draft_path.as_ref() else {
            return;
        };
        let Ok(saved) = fs::read_to_string(path) else {
            return;
        };
        if saved.trim().is_empty() {
            return;
        }
        self.textarea.insert_str(saved);
        self.last_saved = self.draft_text();
    }

    /// Persist the current draft when it changed since the last save.
    /// Failures are ignored: autosave must never interrupt editing.
    fn autosave_draft(&mut self) {
        let Some(path) = self.draft_path.clone() else {
            return;
        };
        let text = self.draft_text();
        if text.trim().is_empty() {
            // Nothing to keep; drop any stale autosave.
            let _ = fs::remove_file(&path);
            self.last_saved = text;
            return;
        }
        if text == self.last_saved {
            return;
        }
        if let Some(parent) = path.parent()
            && fs::create_dir_all(parent).is_err()
        {
            return;
        }
        if fs::write(&path, &text).is_ok() {
            self.last_saved = text;
        }
    }

    /// Delete the autosave file, if any. Used when the draft is published,
    /// explicitly cleared, or deliberately discarded.
    fn delete_autosave(&mut self) {
        if let Some(path) = self.draft_path.as_ref() {
            let _ = fs::remove_file(path);
        }
        self.last_saved = self.draft_text();
    }

    fn focus_next(&mut self) {
        let can_clear = !self.textarea.is_empty();
        let can_publish = self.has_publishable_text();
        self.focus = match self.focus {
            Focus::Textbox => Focus::HelpButton,
            Focus::HelpButton if can_clear => Focus::ClearButton,
            Focus::HelpButton if can_publish => Focus::SaveDraftButton,
            Focus::HelpButton => Focus::Textbox,
            Focus::ClearButton if can_publish => Focus::SaveDraftButton,
            Focus::ClearButton => Focus::Textbox,
            Focus::SaveDraftButton if can_publish => Focus::PostButton,
            Focus::SaveDraftButton => Focus::Textbox,
            Focus::PostButton => Focus::Textbox,
        };
    }

    fn focus_previous(&mut self) {
        let can_clear = !self.textarea.is_empty();
        let can_publish = self.has_publishable_text();
        self.focus = match self.focus {
            Focus::Textbox if can_publish => Focus::PostButton,
            Focus::Textbox if can_clear => Focus::ClearButton,
            Focus::Textbox => Focus::HelpButton,
            Focus::HelpButton => Focus::Textbox,
            Focus::ClearButton => Focus::HelpButton,
            Focus::SaveDraftButton if can_clear => Focus::ClearButton,
            Focus::SaveDraftButton => Focus::HelpButton,
            Focus::PostButton if can_publish => Focus::SaveDraftButton,
            Focus::PostButton if can_clear => Focus::ClearButton,
            Focus::PostButton => Focus::HelpButton,
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_draft_quits_without_confirmation() {
        let mut app = App::default();

        app.handle_key_event(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));

        assert!(app.should_quit);
        assert!(!app.quit_dialog);
    }

    #[test]
    fn draft_opens_confirmation_and_cancel_keeps_app_running() {
        let mut app = App::default();
        app.textarea.insert_str("draft");

        app.handle_key_event(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert!(app.quit_dialog);
        assert!(!app.should_quit);

        app.handle_key_event(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::NONE));

        assert!(!app.quit_dialog);
        assert!(!app.should_quit);
    }

    #[test]
    fn discard_accelerator_and_mouse_button_quit_with_draft() {
        let mut app = App::default();
        app.draft_path = None;
        app.textarea.insert_str("draft");
        app.handle_key_event(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));

        app.handle_key_event(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE));

        assert!(app.should_quit);
        assert!(!app.quit_dialog);

        let mut app = App::default();
        app.draft_path = None;
        app.textarea.insert_str("draft");
        app.handle_key_event(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        app.quit_dialog_areas.discard_button = Rect::new(10, 10, 10, 3);

        app.handle_quit_dialog_mouse_event(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 12,
            row: 11,
            modifiers: KeyModifiers::NONE,
        });

        assert!(app.should_quit);
        assert!(!app.quit_dialog);
    }

    #[test]
    fn top_row_buttons_follow_visual_focus_order() {
        let mut app = App::default();
        app.textarea.insert_str("draft");

        app.handle_key_event(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        assert_eq!(app.focus, Focus::HelpButton);
        app.handle_key_event(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        assert_eq!(app.focus, Focus::ClearButton);
        app.handle_key_event(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        assert_eq!(app.focus, Focus::SaveDraftButton);
        app.handle_key_event(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        assert_eq!(app.focus, Focus::PostButton);
    }

    #[test]
    fn empty_textarea_skips_and_blocks_clear_button() {
        let mut app = App::default();

        app.handle_key_event(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        app.handle_key_event(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        assert_eq!(app.focus, Focus::Textbox);

        app.control_areas.clear_button = Rect::new(10, 1, 9, 3);
        app.focus = Focus::HelpButton;
        app.handle_mouse_event(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 12,
            row: 2,
            modifiers: KeyModifiers::NONE,
        });
        assert_eq!(app.focus, Focus::HelpButton);
        assert!(app.textarea.is_empty());
    }

    #[test]
    fn nonempty_textarea_enables_clear_button() {
        let mut app = App::default();
        app.textarea.insert_str(" ");

        app.handle_key_event(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        app.handle_key_event(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        assert_eq!(app.focus, Focus::ClearButton);

        app.handle_key_event(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert!(app.textarea.is_empty());
        assert_eq!(app.focus, Focus::Textbox);
    }

    #[test]
    fn publish_buttons_skip_whitespace_only_content() {
        let mut app = App::default();
        app.textarea.insert_str(" \n\t");

        app.handle_key_event(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        app.handle_key_event(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        app.handle_key_event(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));

        assert_eq!(app.focus, Focus::Textbox);
        assert!(app.publish_dialog.is_none());
    }

    #[test]
    fn publish_buttons_open_the_matching_confirmation_dialog() {
        let mut app = App::default();
        app.textarea.insert_str("draft");
        // Never hit the network from tests: confirming must fail fast.
        app.service_api_url = None;
        app.service_auth_token = None;

        app.handle_key_event(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        app.handle_key_event(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        app.handle_key_event(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        app.handle_key_event(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

        assert!(matches!(app.publish_dialog, Some(PublishTarget::Draft)));
        app.handle_key_event(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::NONE));
        assert!(app.publish_dialog.is_none());

        app.handle_key_event(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        app.handle_key_event(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert!(matches!(app.publish_dialog, Some(PublishTarget::Post)));

        app.handle_key_event(KeyEvent::new(KeyCode::Char('p'), KeyModifiers::NONE));
        assert!(app.publish_dialog.is_none());
        assert_eq!(app.focus, Focus::Textbox);

        // The unconfigured publish failed with a modal alert; dismiss it
        // before continuing.
        assert!(app.alert.is_some());
        app.handle_key_event(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert!(app.alert.is_none());

        app.control_areas.post_button = Rect::new(10, 1, 16, 3);
        app.handle_mouse_event(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 12,
            row: 2,
            modifiers: KeyModifiers::NONE,
        });
        assert!(matches!(app.publish_dialog, Some(PublishTarget::Post)));
    }

    #[test]
    fn ctrl_enter_opens_post_confirmation_from_textbox() {
        let mut app = App::default();
        app.textarea.insert_str("post");

        app.handle_key_event(KeyEvent::new(KeyCode::Enter, KeyModifiers::CONTROL));

        assert!(matches!(app.publish_dialog, Some(PublishTarget::Post)));
    }

    #[test]
    fn textarea_uses_soft_wrapping() {
        let app = App::default();

        assert_eq!(app.textarea.wrap_mode(), WrapMode::WordOrGlyph);
    }

    #[test]
    fn alt_ctrl_enter_opens_draft_confirmation_from_textbox() {
        let mut app = App::default();
        app.textarea.insert_str("draft");

        app.handle_key_event(KeyEvent::new(
            KeyCode::Enter,
            KeyModifiers::ALT | KeyModifiers::CONTROL,
        ));

        assert!(matches!(app.publish_dialog, Some(PublishTarget::Draft)));
    }

    #[test]
    fn ctrl_z_undoes_and_ctrl_y_redoes() {
        let mut app = App::default();
        app.draft_path = None;
        app.textarea.insert_str("draft");

        app.clear_input();
        assert!(app.textarea.is_empty());

        app.handle_key_event(KeyEvent::new(KeyCode::Char('z'), KeyModifiers::CONTROL));
        assert_eq!(app.textarea.lines(), ["draft"]);

        app.handle_key_event(KeyEvent::new(KeyCode::Char('y'), KeyModifiers::CONTROL));

        assert!(app.textarea.is_empty());
        assert_eq!(app.focus, Focus::Textbox);
    }

    #[test]
    fn ctrl_v_pastes_from_the_textarea_yank_buffer() {
        let mut app = App::default();
        app.draft_path = None;
        app.textarea.insert_str("draft");
        app.textarea.select_all();
        app.textarea.copy();
        app.clear_input();

        app.handle_key_event(KeyEvent::new(KeyCode::Char('v'), KeyModifiers::CONTROL));

        assert_eq!(app.textarea.lines(), ["draft"]);
    }

    #[test]
    fn help_command_toggles_sidebar_with_keyboard_and_mouse() {
        let mut app = App::default();

        app.handle_key_event(KeyEvent::new(KeyCode::F(1), KeyModifiers::NONE));
        assert!(app.help_sidebar);

        app.handle_key_event(KeyEvent::new(KeyCode::F(1), KeyModifiers::NONE));
        assert!(!app.help_sidebar);

        app.control_areas.help_button = Rect::new(10, 1, 10, 3);
        app.handle_mouse_event(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 12,
            row: 2,
            modifiers: KeyModifiers::NONE,
        });
        assert!(app.help_sidebar);

        app.handle_mouse_event(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 12,
            row: 2,
            modifiers: KeyModifiers::NONE,
        });
        assert!(!app.help_sidebar);
    }

    #[test]
    fn confirming_publish_starts_background_job() {
        let mut app = App::default();
        app.textarea.insert_str("draft");
        // Point at an unroutable address so the worker fails fast
        // without touching the network.
        app.service_api_url = Some("http://127.0.0.1:9/micropub".to_string());
        app.service_auth_token = Some("test-token".to_string());
        app.open_publish_dialog(PublishTarget::Draft);

        app.publish_dialog_focus = PublishDialogFocus::Publish;
        app.activate_publish_dialog_focus();

        assert!(app.publish_dialog.is_none());
        assert!(matches!(
            app.publish_job.as_ref().map(|job| job.target),
            Some(PublishTarget::Draft)
        ));
        assert_eq!(app.focus, Focus::Textbox);
        assert!(app.alert.is_none());
        assert!(!app.textarea.is_empty());
        // Drop the job so the stray worker result is discarded.
        app.publish_job = None;
    }

    #[test]
    fn publishing_without_configured_endpoint_alerts_and_keeps_text() {
        let mut app = App::default();
        app.textarea.insert_str("draft");
        app.service_api_url = None;
        app.service_auth_token = None;
        app.open_publish_dialog(PublishTarget::Post);

        app.publish_dialog_focus = PublishDialogFocus::Publish;
        app.activate_publish_dialog_focus();

        assert!(app.publish_dialog.is_none());
        assert!(app.publish_job.is_none());
        assert!(app.alert.is_some());
        assert_eq!(app.alert.as_ref().map(|a| a.kind), Some(AlertKind::Error));
        assert_eq!(app.textarea.lines(), ["draft"]);
    }

    #[test]
    fn publishing_without_auth_token_alerts_and_keeps_text() {
        let mut app = App::default();
        app.textarea.insert_str("draft");
        app.service_api_url = Some("https://example.com/micropub".to_string());
        app.service_auth_token = None;
        app.open_publish_dialog(PublishTarget::Post);

        app.publish_dialog_focus = PublishDialogFocus::Publish;
        app.activate_publish_dialog_focus();

        assert!(app.publish_job.is_none());
        assert_eq!(app.alert.as_ref().map(|a| a.kind), Some(AlertKind::Error));
        assert_eq!(app.textarea.lines(), ["draft"]);
    }

    fn test_draft_path(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("mptui-ui-test-{}-{name}", std::process::id()))
    }

    #[test]
    fn status_text_shows_counts_and_cursor() {
        let mut app = App::new(MicropubSettings::default(), None);
        app.textarea.insert_str("hello world\nfoo");

        let status = app.status_text();
        assert!(status.contains("Words: 3"), "unexpected status: {status}");
        assert!(status.contains("Chars: 15"), "unexpected status: {status}");
        assert!(status.contains("Ln:2,Col:4"), "unexpected status: {status}");
    }

    #[test]
    fn status_text_shows_publishing_state_and_last_url() {
        let mut app = App::new(MicropubSettings::default(), None);
        let (_sender, receiver) = mpsc::channel();
        app.publish_job = Some(PublishJob {
            target: PublishTarget::Post,
            receiver,
        });
        assert_eq!(app.status_text(), "Publishing...");

        app.publish_job = None;
        app.last_published =
            Some("https://micro.blog/some/very/long/post/url/that/keeps/going".to_string());
        let status = app.status_text();
        assert!(
            status.contains("Last: https://micro.blog/some/very/lon..."),
            "unexpected status: {status}"
        );
    }

    #[test]
    fn truncate_end_keeps_short_text_and_cuts_long_text() {
        assert_eq!(truncate_end("abc", 5), "abc");
        assert_eq!(truncate_end("abcde", 5), "abcde");
        assert_eq!(truncate_end("abcdef", 5), "abcde...");
    }

    #[test]
    fn parse_settings_reports_configured_endpoint() {
        let settings = parse_micropub_settings(
            "[service]\napi_url = \"https://example.com/micropub\"\nauth_token = \"secret\"\n",
        );

        assert_eq!(
            settings.endpoint,
            EndpointState::Configured("example.com/micropub".to_string())
        );
        assert_eq!(
            settings.service_api_url.as_deref(),
            Some("https://example.com/micropub")
        );
        assert_eq!(settings.service_auth_token.as_deref(), Some("secret"));
    }

    #[test]
    fn parse_settings_reports_missing_token() {
        let settings =
            parse_micropub_settings("[service]\napi_url = \"https://example.com/micropub\"\n");

        assert_eq!(
            settings.endpoint,
            EndpointState::MissingToken("example.com/micropub".to_string())
        );
        assert_eq!(
            settings.service_api_url.as_deref(),
            Some("https://example.com/micropub")
        );
        assert_eq!(settings.service_auth_token, None);
    }

    #[test]
    fn parse_settings_reports_missing_endpoint() {
        let settings = parse_micropub_settings("[service]\nauth_token = \"secret\"\n");
        assert_eq!(settings.endpoint, EndpointState::Missing);

        let settings = parse_micropub_settings("");
        assert_eq!(settings.endpoint, EndpointState::Missing);

        let settings = parse_micropub_settings("[service]\napi_url = \"   \"\n");
        assert_eq!(settings.endpoint, EndpointState::Missing);
    }

    #[test]
    fn parse_settings_reports_invalid_toml() {
        let settings = parse_micropub_settings("[service\napi_url = ");
        assert!(matches!(settings.endpoint, EndpointState::ParseError(_)));
    }

    #[test]
    fn parse_settings_reads_title_extraction() {
        let settings = parse_micropub_settings("[default_behavior]\nextract_title = true\n");
        assert!(settings.extract_title);

        let settings = parse_micropub_settings("");
        assert!(!settings.extract_title);
    }

    #[test]
    fn apply_settings_replaces_endpoint_and_credentials() {
        let mut app = App::new(MicropubSettings::default(), None);
        assert_eq!(app.endpoint, EndpointState::Missing);

        app.apply_settings(parse_micropub_settings(
            "[service]\napi_url = \"https://example.com/micropub\"\nauth_token = \"secret\"\n[default_behavior]\nextract_title = true\n",
        ));

        assert_eq!(
            app.endpoint,
            EndpointState::Configured("example.com/micropub".to_string())
        );
        assert_eq!(
            app.service_api_url.as_deref(),
            Some("https://example.com/micropub")
        );
        assert_eq!(app.service_auth_token.as_deref(), Some("secret"));
        assert!(app.extract_title);
    }

    #[test]
    fn auth_failures_are_detected_for_config_reload() {
        assert!(is_auth_failure("API error: unauthorized - bad token"));
        assert!(is_auth_failure("API error: invalid_token - expired"));
        assert!(is_auth_failure("request failed with status 401"));
        assert!(!is_auth_failure(
            "API error: invalid_request - missing content"
        ));
        assert!(!is_auth_failure("network unreachable"));
    }

    #[test]
    fn autosave_round_trip_restores_draft() {
        let path = test_draft_path("round-trip");
        let _ = fs::remove_file(&path);

        let mut app = App::new(MicropubSettings::default(), Some(path.clone()));
        app.textarea.insert_str("line one\nline two");
        app.autosave_draft();
        assert_eq!(
            fs::read_to_string(&path).expect("autosave should write the draft"),
            "line one\nline two"
        );

        let mut restored = App::new(MicropubSettings::default(), Some(path.clone()));
        restored.restore_draft();
        assert_eq!(restored.textarea.lines(), ["line one", "line two"]);

        let _ = fs::remove_file(&path);
    }

    #[test]
    fn autosave_skips_unchanged_text() {
        let path = test_draft_path("unchanged");
        let _ = fs::remove_file(&path);

        let mut app = App::new(MicropubSettings::default(), Some(path.clone()));
        app.textarea.insert_str("same");
        app.autosave_draft();
        assert!(path.exists());

        // No changes since the last save: the file must be left alone.
        fs::remove_file(&path).expect("autosave should have written the draft");
        app.autosave_draft();
        assert!(!path.exists());
    }

    #[test]
    fn autosave_removes_stale_file_for_empty_text() {
        let path = test_draft_path("stale");
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("test setup should create the parent dir");
        }
        fs::write(&path, "stale").expect("test setup should write the stale file");

        let mut app = App::new(MicropubSettings::default(), Some(path.clone()));
        app.autosave_draft();

        assert!(!path.exists());
    }

    #[test]
    fn clear_input_deletes_autosave() {
        let path = test_draft_path("clear");
        let _ = fs::remove_file(&path);

        let mut app = App::new(MicropubSettings::default(), Some(path.clone()));
        app.textarea.insert_str("draft");
        app.autosave_draft();
        assert!(path.exists());

        app.clear_input();
        assert!(!path.exists());
    }

    #[test]
    fn discard_quit_deletes_autosave() {
        let path = test_draft_path("discard");
        let _ = fs::remove_file(&path);

        let mut app = App::new(MicropubSettings::default(), Some(path.clone()));
        app.textarea.insert_str("draft");
        app.autosave_draft();
        assert!(path.exists());

        app.quit_dialog = true;
        app.quit_dialog_focus = QuitDialogFocus::Discard;
        app.activate_quit_dialog_focus();

        assert!(app.should_quit);
        assert!(!path.exists());
    }

    #[test]
    fn alert_key_and_mouse_input_dismiss_the_dialog() {
        let mut app = App::default();
        app.alert = Some(Alert::published("Post published successfully.".to_string()));

        app.handle_key_event(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE));
        assert!(app.alert.is_some());

        app.handle_key_event(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert!(app.alert.is_none());

        app.alert = Some(Alert::published(
            "Draft published successfully.".to_string(),
        ));
        app.handle_key_event(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert!(app.alert.is_none());

        app.alert = Some(Alert::error("Failed to publish post.".to_string()));
        app.alert_button_area = Rect::new(10, 10, 6, 3);
        app.handle_mouse_event(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 12,
            row: 11,
            modifiers: KeyModifiers::NONE,
        });
        assert!(app.alert.is_none());

        app.alert = Some(Alert::error("Failed to publish post.".to_string()));
        app.handle_mouse_event(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 0,
            row: 0,
            modifiers: KeyModifiers::NONE,
        });
        assert!(app.alert.is_some());
    }
}
