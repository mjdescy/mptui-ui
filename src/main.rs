use std::{io, time::Duration};

use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::{
    DefaultTerminal, Frame,
    layout::{Alignment, Constraint, Layout, Position},
    style::{Color, Modifier, Style},
    symbols::border,
    widgets::{Block, Paragraph},
};

fn main() -> io::Result<()> {
    ratatui::run(|terminal| App::default().run(terminal))
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Focus {
    Textbox,
    PostButton,
    ClearButton,
}

struct App {
    input: Vec<char>,
    cursor_position: usize,
    focus: Focus,
    should_quit: bool,
}

impl Default for App {
    fn default() -> Self {
        Self {
            input: Vec::new(),
            cursor_position: 0,
            focus: Focus::Textbox,
            should_quit: false,
        }
    }
}

impl App {
    fn run(mut self, terminal: &mut DefaultTerminal) -> io::Result<()> {
        while !self.should_quit {
            terminal.draw(|frame| self.draw(frame))?;
            self.handle_events()?;
        }

        Ok(())
    }

    fn draw(&self, frame: &mut Frame) {
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

        let input: String = self.input.iter().collect();
        let visible_width = areas[1].width.saturating_sub(2) as usize;
        let scroll = if visible_width == 0 {
            0
        } else {
            self.cursor_position
                .saturating_sub(visible_width.saturating_sub(1))
        };
        let textbox_style = if self.focus == Focus::Textbox {
            Style::default().fg(Color::Cyan)
        } else {
            Style::default().fg(Color::DarkGray)
        };
        let textbox = Paragraph::new(input).scroll((0, scroll as u16)).block(
            Block::bordered()
                .border_set(border::ROUNDED)
                .border_style(textbox_style),
        );
        frame.render_widget(textbox, areas[1]);

        let button_areas = Layout::horizontal([
            Constraint::Fill(1),
            Constraint::Length(12),
            Constraint::Length(12),
            Constraint::Fill(1),
        ])
        .spacing(1)
        .split(areas[2]);
        self.render_button(frame, button_areas[1], "Post", Focus::PostButton);
        self.render_button(frame, button_areas[2], "Clear", Focus::ClearButton);

        if self.focus == Focus::Textbox && areas[1].width > 2 && areas[1].height > 2 {
            let cursor_x = areas[1].x + 1 + (self.cursor_position - scroll) as u16;
            frame.set_cursor_position(Position::new(cursor_x, areas[1].y + 1));
        }
    }

    fn render_button(
        &self,
        frame: &mut Frame,
        area: ratatui::layout::Rect,
        label: &str,
        focus: Focus,
    ) {
        let style = if self.focus == focus {
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
        if event::poll(Duration::from_millis(250))?
            && let Event::Key(key_event) = event::read()?
            && key_event.kind == KeyEventKind::Press
        {
            self.handle_key_event(key_event);
        }

        Ok(())
    }

    fn handle_key_event(&mut self, key_event: KeyEvent) {
        if key_event.code == KeyCode::Esc
            || (key_event.code == KeyCode::Char('c')
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
            _ if self.focus == Focus::Textbox => self.handle_text_input(key_event.code),
            _ => {}
        }
    }

    fn handle_text_input(&mut self, key_code: KeyCode) {
        match key_code {
            KeyCode::Char(character) => {
                self.input.insert(self.cursor_position, character);
                self.cursor_position += 1;
            }
            KeyCode::Backspace if self.cursor_position > 0 => {
                self.cursor_position -= 1;
                self.input.remove(self.cursor_position);
            }
            KeyCode::Delete if self.cursor_position < self.input.len() => {
                self.input.remove(self.cursor_position);
            }
            KeyCode::Left if self.cursor_position > 0 => self.cursor_position -= 1,
            KeyCode::Right if self.cursor_position < self.input.len() => self.cursor_position += 1,
            KeyCode::Home => self.cursor_position = 0,
            KeyCode::End => self.cursor_position = self.input.len(),
            KeyCode::Enter => self.focus = Focus::PostButton,
            _ => {}
        }
    }

    fn clear_input(&mut self) {
        self.input.clear();
        self.cursor_position = 0;
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
