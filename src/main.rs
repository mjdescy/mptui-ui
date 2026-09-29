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
    widgets::{Block, Paragraph},
};
use tui_textarea::TextArea;

fn main() -> io::Result<()> {
    let mut terminal = ratatui::init();
    let result = App::default().run(&mut terminal);
    ratatui::restore();
    result
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Focus {
    Textbox,
    PostButton,
    ClearButton,
}

struct App {
    textarea: TextArea<'static>,
    focus: Focus,
    hovered: Option<Focus>,
    should_quit: bool,
    control_areas: ControlAreas,
}

#[derive(Default)]
struct ControlAreas {
    textbox: Rect,
    post_button: Rect,
    clear_button: Rect,
}

impl Default for App {
    fn default() -> Self {
        Self {
            textarea: TextArea::default(),
            focus: Focus::Textbox,
            hovered: None,
            should_quit: false,
            control_areas: ControlAreas::default(),
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
            Constraint::Length(1),
            Constraint::Min(5),
            Constraint::Length(3),
        ])
        .split(frame.area());

        let label = Paragraph::new("Post").style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        );
        frame.render_widget(label, areas[0]);

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
            Constraint::Fill(1),
            Constraint::Length(12),
            Constraint::Length(12),
            Constraint::Fill(1),
        ])
        .spacing(1)
        .split(areas[2]);
        self.control_areas = ControlAreas {
            textbox: areas[1],
            post_button: button_areas[1],
            clear_button: button_areas[2],
        };
        self.render_button(frame, button_areas[1], "Post", Focus::PostButton);
        self.render_button(frame, button_areas[2], "Clear", Focus::ClearButton);
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
        let position = Position::new(mouse_event.column, mouse_event.row);
        self.hovered = if self.control_areas.post_button.contains(position) {
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
        } else if self.control_areas.post_button.contains(position) {
            self.focus = Focus::Textbox;
        } else if self.control_areas.clear_button.contains(position) {
            self.clear_input();
        }
    }

    fn handle_key_event(&mut self, key_event: KeyEvent) {
        if key_event.code == KeyCode::Esc
            || (key_event.code == KeyCode::Char('q')
                && key_event.modifiers.contains(KeyModifiers::CONTROL))
        {
            self.should_quit = true;
            return;
        }

        match key_event.code {
            KeyCode::Tab => self.focus_next(),
            KeyCode::BackTab => self.focus_previous(),
            KeyCode::Enter if self.focus == Focus::ClearButton => self.clear_input(),
            KeyCode::Enter if self.focus == Focus::PostButton => self.focus = Focus::Textbox,
            _ if self.focus == Focus::Textbox => {
                self.textarea.input(key_event);
            }
            _ => {}
        }
    }

    fn clear_input(&mut self) {
        self.textarea = TextArea::default();
        self.focus = Focus::Textbox;
    }

    fn focus_next(&mut self) {
        self.focus = match self.focus {
            Focus::Textbox => Focus::PostButton,
            Focus::PostButton => Focus::ClearButton,
            Focus::ClearButton => Focus::Textbox,
        };
    }

    fn focus_previous(&mut self) {
        self.focus = match self.focus {
            Focus::Textbox => Focus::ClearButton,
            Focus::PostButton => Focus::Textbox,
            Focus::ClearButton => Focus::PostButton,
        };
    }
}
