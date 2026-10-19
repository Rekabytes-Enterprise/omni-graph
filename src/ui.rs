//! Startup choice; explicit subcommands remain usable without a terminal.
use crate::cli::{Command, Web};
use anyhow::{Result, ensure};
use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use ratatui::{
    layout::{Constraint, Layout},
    style::{Color, Modifier, Style},
    widgets::{Block, List, ListItem, ListState, Paragraph},
};
use std::io::{self, IsTerminal};

pub fn choose() -> Result<Option<Command>> {
    ensure!(
        io::stdin().is_terminal() && io::stdout().is_terminal(),
        "interface selection needs an interactive terminal; use `omni-graph web --no-open`, `omni-graph tui`, or `omni-graph graph --json`"
    );
    Ok(ratatui::run(|terminal| -> io::Result<Option<Command>> {
        let mut selected = 0;
        loop {
            terminal.draw(|frame| {
                let area = frame.area();
                frame.render_widget(
                    Block::default()
                        .style(Style::default().bg(Color::Rgb(35, 37, 42)).fg(Color::White)),
                    area,
                );
                let rows = Layout::vertical([
                    Constraint::Fill(1),
                    Constraint::Length(2),
                    Constraint::Length(5),
                    Constraint::Length(2),
                    Constraint::Fill(1),
                ])
                .split(area);
                let width = Layout::horizontal([
                    Constraint::Fill(1),
                    Constraint::Max(68),
                    Constraint::Fill(1),
                ]);
                frame.render_widget(
                    Paragraph::new("OMNI-GRAPH  /  Choose your view").style(
                        Style::default()
                            .fg(Color::Cyan)
                            .add_modifier(Modifier::BOLD),
                    ),
                    width.split(rows[1])[1],
                );
                let items = [
                    ListItem::new("1  Web UI   · Browser graph at 127.0.0.1:52716"),
                    ListItem::new("2  TUI      · Stay in your terminal"),
                    ListItem::new("3  Remote   · HTTPS link with email/PIN protection"),
                ];
                frame.render_stateful_widget(
                    List::new(items).highlight_symbol("› ").highlight_style(
                        Style::default()
                            .bg(Color::Rgb(62, 69, 80))
                            .add_modifier(Modifier::BOLD),
                    ),
                    width.split(rows[2])[1],
                    &mut ListState::default().with_selected(Some(selected)),
                );
                frame.render_widget(
                    Paragraph::new("↑ ↓ select   Enter open   Esc quit"),
                    width.split(rows[3])[1],
                );
            })?;
            if let Event::Key(key) = event::read()? {
                if key.kind != KeyEventKind::Press {
                    continue;
                }
                match key.code {
                    KeyCode::Esc | KeyCode::Char('q') => return Ok(None),
                    KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        return Ok(None);
                    }
                    KeyCode::Up | KeyCode::Down | KeyCode::Tab | KeyCode::Char('j' | 'k') => {
                        selected = if key.code == KeyCode::Up || key.code == KeyCode::Char('k') {
                            (selected + 2) % 3
                        } else {
                            (selected + 1) % 3
                        }
                    }
                    KeyCode::Char('1') => {
                        return Ok(Some(Command::Web(Web::default())));
                    }
                    KeyCode::Char('2') => return Ok(Some(Command::Tui)),
                    KeyCode::Char('3') => {
                        return Ok(Some(Command::Web(Web {
                            remote: true,
                            ..Web::default()
                        })));
                    }
                    KeyCode::Enter => {
                        return Ok(Some(if selected == 0 {
                            Command::Web(Web::default())
                        } else if selected == 1 {
                            Command::Tui
                        } else {
                            Command::Web(Web {
                                remote: true,
                                ..Web::default()
                            })
                        }));
                    }
                    _ => (),
                }
            }
        }
    })?)
}
