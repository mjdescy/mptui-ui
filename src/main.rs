use std::{io, time::Duration};

use crossterm::{
    event::{
        self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent,
        MouseEventKind,
    },
    execute,
};
use ratatui::{
    DefaultTerminal, Frame,
    layout::{Alignment, Constraint, Layout, Position, Rect},
    style::{Color, Modifier, Style},
    symbols::border,
    widgets::{Block, Clear, Paragraph},
};
use tui_textarea::TextArea;

fn main() -> io::Result<()> {
    let mut terminal = ratatui::init();
    let result = App::default().run(&mut terminal);
    ratatui::restore();
    result
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Focus {
    Textbox,
    PostMode,
    DraftMode,
    PostButton,
    ClearButton,
    HelpButton,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ComposeMode {
    Post,
    Draft,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum QuitDialogFocus {
    Cancel,
    Discard,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum HelpDialogFocus {
    Close,
}

struct App {
    textarea: TextArea<'static>,
    focus: Focus,
    compose_mode: ComposeMode,
    hovered: Option<Focus>,
    should_quit: bool,
    quit_dialog: bool,
    quit_dialog_focus: QuitDialogFocus,
    help_dialog: bool,
    help_dialog_focus: HelpDialogFocus,
    control_areas: ControlAreas,
    quit_dialog_areas: QuitDialogAreas,
    help_dialog_areas: HelpDialogAreas,
}

#[derive(Default)]
struct ControlAreas {
    textbox: Rect,
    help_button: Rect,
    post_mode: Rect,
    draft_mode: Rect,
    post_button: Rect,
    clear_button: Rect,
}

#[derive(Default)]
struct QuitDialogAreas {
    cancel_button: Rect,
    discard_button: Rect,
}

#[derive(Default)]
struct HelpDialogAreas {
    close_button: Rect,
}

impl Default for App {
    fn default() -> Self {
        Self {
            textarea: TextArea::default(),
            focus: Focus::Textbox,
            compose_mode: ComposeMode::Post,
            hovered: None,
            should_quit: false,
            quit_dialog: false,
            quit_dialog_focus: QuitDialogFocus::Cancel,
            help_dialog: false,
            help_dialog_focus: HelpDialogFocus::Close,
            control_areas: ControlAreas::default(),
            quit_dialog_areas: QuitDialogAreas::default(),
            help_dialog_areas: HelpDialogAreas::default(),
        }
    }
}

impl App {
    fn run(mut self, terminal: &mut DefaultTerminal) -> io::Result<()> {
        execute!(io::stdout(), event::EnableMouseCapture)?;

        let result = self.run_loop(terminal);
        let disable_result = execute!(io::stdout(), event::DisableMouseCapture);

        result.and(disable_result)
    }

    fn run_loop(&mut self, terminal: &mut DefaultTerminal) -> io::Result<()> {
        while !self.should_quit {
            terminal.draw(|frame| self.draw(frame))?;
            self.handle_events()?;
        }

        Ok(())
    }

    fn draw(&mut self, frame: &mut Frame) {
        let areas = Layout::vertical([
            Constraint::Length(3),
            Constraint::Min(5),
            Constraint::Length(3),
            Constraint::Length(1),
        ])
        .split(frame.area());

        let header_areas = Layout::horizontal([Constraint::Min(1), Constraint::Length(10)])
            .spacing(1)
            .split(areas[0]);
        let label = Paragraph::new("MPTUI")
            .style(
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )
            .alignment(Alignment::Left);
        frame.render_widget(label, header_areas[0]);
        self.control_areas.help_button = header_areas[1];
        self.render_button(frame, header_areas[1], "Help", Focus::HelpButton);

        let textbox_style = if self.focus == Focus::Textbox {
            Style::default().fg(Color::Cyan)
        } else {
            Style::default().fg(Color::DarkGray)
        };
        let cursor_line_style = if self.focus == Focus::Textbox {
            Style::default().bg(Color::Rgb(35, 45, 52))
        } else {
            Style::default()
        };
        self.textarea.set_cursor_line_style(cursor_line_style);
        self.textarea.set_block(
            Block::bordered()
                .border_set(border::ROUNDED)
                .border_style(textbox_style),
        );
        frame.render_widget(&self.textarea, areas[1]);

        let button_areas = Layout::horizontal([
            Constraint::Length(10),
            Constraint::Length(11),
            Constraint::Fill(1),
            Constraint::Length(12),
            Constraint::Length(12),
            Constraint::Fill(1),
        ])
        .spacing(1)
        .split(areas[2]);
        self.control_areas = ControlAreas {
            textbox: areas[1],
            help_button: header_areas[1],
            post_mode: button_areas[0],
            draft_mode: button_areas[1],
            post_button: button_areas[3],
            clear_button: button_areas[4],
        };
        self.render_radio(
            frame,
            button_areas[0],
            "Post",
            ComposeMode::Post,
            Focus::PostMode,
        );
        self.render_radio(
            frame,
            button_areas[1],
            "Draft",
            ComposeMode::Draft,
            Focus::DraftMode,
        );
        self.render_button(frame, button_areas[3], "Post", Focus::PostButton);
        self.render_button(frame, button_areas[4], "Clear", Focus::ClearButton);

        let commands =
            Paragraph::new("F1 Help   Tab/Shift+Tab Navigate   Enter/Space Select   Esc Quit")
                .style(Style::default().fg(Color::DarkGray))
                .alignment(Alignment::Center);
        frame.render_widget(commands, areas[3]);

        if self.quit_dialog {
            self.draw_quit_dialog(frame);
        } else if self.help_dialog {
            self.draw_help_dialog(frame);
        }
    }

    fn draw_help_dialog(&mut self, frame: &mut Frame) {
        let dialog_area = Layout::vertical([
            Constraint::Percentage(25),
            Constraint::Percentage(50),
            Constraint::Percentage(25),
        ])
        .split(frame.area())[1];
        let dialog_area = Layout::horizontal([
            Constraint::Percentage(20),
            Constraint::Percentage(60),
            Constraint::Percentage(20),
        ])
        .split(dialog_area)[1];

        let dialog = Block::bordered()
            .title(" Help ")
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
        let message = Paragraph::new("Help content goes here.").alignment(Alignment::Center);
        frame.render_widget(message, content[1]);

        self.help_dialog_areas.close_button = content[2];
        self.render_help_button(frame, content[2], "Close");
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
        let style = if self.focus == focus || self.hovered == Some(focus) {
            Style::default()
                .fg(Color::Black)
                .bg(Color::Cyan)
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

    fn render_radio(
        &self,
        frame: &mut Frame,
        area: Rect,
        label: &str,
        mode: ComposeMode,
        focus: Focus,
    ) {
        let is_active = self.compose_mode == mode;
        let style = if self.focus == focus || self.hovered == Some(focus) {
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD)
        } else if is_active {
            Style::default().fg(Color::Cyan)
        } else {
            Style::default().fg(Color::White)
        };
        let marker = if is_active { "(*)" } else { "( )" };
        let radio = Paragraph::new(format!("{marker} {label}"))
            .style(style)
            .alignment(Alignment::Left);
        frame.render_widget(radio, area);
    }

    fn render_help_button(&self, frame: &mut Frame, area: Rect, label: &str) {
        let style = Style::default()
            .fg(Color::Black)
            .bg(Color::Cyan)
            .add_modifier(Modifier::BOLD);
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
        if event::poll(Duration::from_millis(250))? {
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
        if self.quit_dialog {
            self.handle_quit_dialog_mouse_event(mouse_event);
            return;
        }
        if self.help_dialog {
            self.handle_help_dialog_mouse_event(mouse_event);
            return;
        }

        let position = Position::new(mouse_event.column, mouse_event.row);
        self.hovered = if self.control_areas.help_button.contains(position) {
            Some(Focus::HelpButton)
        } else if self.control_areas.post_mode.contains(position) {
            Some(Focus::PostMode)
        } else if self.control_areas.draft_mode.contains(position) {
            Some(Focus::DraftMode)
        } else if self.control_areas.post_button.contains(position) {
            Some(Focus::PostButton)
        } else if self.control_areas.clear_button.contains(position) {
            Some(Focus::ClearButton)
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
            self.help_dialog = true;
            self.help_dialog_focus = HelpDialogFocus::Close;
        } else if self.control_areas.post_mode.contains(position) {
            self.focus = Focus::PostMode;
            self.compose_mode = ComposeMode::Post;
        } else if self.control_areas.draft_mode.contains(position) {
            self.focus = Focus::DraftMode;
            self.compose_mode = ComposeMode::Draft;
        } else if self.control_areas.post_button.contains(position) {
            self.focus = Focus::Textbox;
        } else if self.control_areas.clear_button.contains(position) {
            self.clear_input();
        }
    }

    fn handle_help_dialog_mouse_event(&mut self, mouse_event: MouseEvent) {
        let position = Position::new(mouse_event.column, mouse_event.row);
        if !self.help_dialog_areas.close_button.contains(position) {
            return;
        }

        self.help_dialog_focus = HelpDialogFocus::Close;
        if mouse_event.kind == MouseEventKind::Down(MouseButton::Left) {
            self.help_dialog = false;
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

    fn handle_key_event(&mut self, key_event: KeyEvent) {
        if self.quit_dialog {
            self.handle_quit_dialog_key_event(key_event);
            return;
        }
        if self.help_dialog {
            self.handle_help_dialog_key_event(key_event);
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
            self.help_dialog = true;
            self.help_dialog_focus = HelpDialogFocus::Close;
            return;
        }

        match key_event.code {
            KeyCode::Tab => self.focus_next(),
            KeyCode::BackTab => self.focus_previous(),
            KeyCode::Enter | KeyCode::Char(' ') if self.focus == Focus::PostMode => {
                self.compose_mode = ComposeMode::Post;
            }
            KeyCode::Enter | KeyCode::Char(' ') if self.focus == Focus::DraftMode => {
                self.compose_mode = ComposeMode::Draft;
            }
            KeyCode::Enter if self.focus == Focus::ClearButton => self.clear_input(),
            KeyCode::Enter if self.focus == Focus::PostButton => self.focus = Focus::Textbox,
            KeyCode::Enter if self.focus == Focus::HelpButton => {
                self.help_dialog = true;
                self.help_dialog_focus = HelpDialogFocus::Close;
            }
            _ if self.focus == Focus::Textbox => {
                self.textarea.input(key_event);
            }
            _ => {}
        }
    }

    fn handle_help_dialog_key_event(&mut self, key_event: KeyEvent) {
        match key_event.code {
            KeyCode::Esc | KeyCode::Enter => self.help_dialog = false,
            KeyCode::Tab | KeyCode::BackTab => {
                self.help_dialog_focus = HelpDialogFocus::Close;
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

    fn activate_quit_dialog_focus(&mut self) {
        match self.quit_dialog_focus {
            QuitDialogFocus::Cancel => self.quit_dialog = false,
            QuitDialogFocus::Discard => {
                self.quit_dialog = false;
                self.should_quit = true;
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

    fn clear_input(&mut self) {
        self.textarea = TextArea::default();
        self.focus = Focus::Textbox;
    }

    fn focus_next(&mut self) {
        self.focus = match self.focus {
            Focus::Textbox => Focus::PostMode,
            Focus::PostMode => Focus::DraftMode,
            Focus::DraftMode => Focus::PostButton,
            Focus::PostButton => Focus::ClearButton,
            Focus::ClearButton => Focus::HelpButton,
            Focus::HelpButton => Focus::Textbox,
        };
    }

    fn focus_previous(&mut self) {
        self.focus = match self.focus {
            Focus::Textbox => Focus::ClearButton,
            Focus::PostMode => Focus::Textbox,
            Focus::DraftMode => Focus::PostMode,
            Focus::PostButton => Focus::DraftMode,
            Focus::ClearButton => Focus::PostButton,
            Focus::HelpButton => Focus::ClearButton,
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
        app.textarea.insert_str("draft");
        app.handle_key_event(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));

        app.handle_key_event(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE));

        assert!(app.should_quit);
        assert!(!app.quit_dialog);

        let mut app = App::default();
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
    fn mode_radio_buttons_support_keyboard_and_mouse_selection() {
        let mut app = App::default();

        app.handle_key_event(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        assert_eq!(app.focus, Focus::PostMode);
        app.handle_key_event(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        app.handle_key_event(KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE));
        assert_eq!(app.focus, Focus::DraftMode);
        assert_eq!(app.compose_mode, ComposeMode::Draft);

        app.control_areas.draft_mode = Rect::new(10, 10, 11, 3);
        app.handle_mouse_event(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 12,
            row: 11,
            modifiers: KeyModifiers::NONE,
        });

        assert_eq!(app.focus, Focus::DraftMode);
        assert_eq!(app.compose_mode, ComposeMode::Draft);
    }

    #[test]
    fn help_button_opens_and_closes_modal_with_keyboard_and_mouse() {
        let mut app = App::default();

        app.handle_key_event(KeyEvent::new(KeyCode::F(1), KeyModifiers::NONE));
        assert!(app.help_dialog);

        app.handle_key_event(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert!(!app.help_dialog);

        app.control_areas.help_button = Rect::new(10, 1, 10, 3);
        app.handle_mouse_event(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 12,
            row: 2,
            modifiers: KeyModifiers::NONE,
        });
        assert!(app.help_dialog);

        app.help_dialog_areas.close_button = Rect::new(10, 10, 10, 3);
        app.handle_mouse_event(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 12,
            row: 11,
            modifiers: KeyModifiers::NONE,
        });
        assert!(!app.help_dialog);
    }
}
