use std::time::{Duration, Instant};

use color_eyre::Result;
use crossterm::event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode};
use crossterm::execute;
use crossterm::terminal::{EnterAlternateScreen, LeaveAlternateScreen};
use itertools::Itertools;
use tui::backend::{Backend, CrosstermBackend};
use tui::layout::{Constraint, Layout, Margin};
use tui::style::{Color, Modifier, Style};
use tui::text::{Span, Spans, Text};
use tui::widgets::{List, ListItem, Paragraph};
use tui::{Frame, Terminal};

use crate::archive::Archive;

use self::statefullist::StatefulList;

fn render_archive<'a>(archive: &'a Archive) -> Text<'a> {
    let title = Span::styled(
        &archive.name,
        Style::default()
            .fg(Color::Rgb(73, 159, 147))
            .add_modifier(Modifier::BOLD),
    );
    let sep = Span::styled(": ", Style::default().fg(Color::Rgb(73, 159, 147)));
    let artist = Span::styled(
        &archive.artist,
        Style::default().fg(Color::Rgb(73, 159, 147)),
    );
    let a = Spans::from(vec![title, sep, artist]);
    let b = Spans::from(
        Itertools::intersperse(
            archive.tags.iter().map(|t| {
                Span::styled(
                    &t.name,
                    Style::default()
                        .fg(Color::Rgb(32, 178, 170))
                        .add_modifier(Modifier::DIM),
                )
            }),
            Span::styled(", ", Style::default().add_modifier(Modifier::DIM)),
        )
        .collect_vec(),
    );

    Text { lines: vec![a, b] }
}

pub fn do_pick<'a>(query: &str, inputs: &'a [Archive]) -> Result<Option<&'a Archive>> {
    crossterm::terminal::enable_raw_mode()?;
    let mut stdout = std::io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    terminal.clear()?;

    let tick_rate = Duration::from_millis(200);
    let mut list = StatefulList::with_items(inputs.iter().map(render_archive).collect_vec());
    list.next();
    let selection = run_app(&mut terminal, query, list, tick_rate)?;

    crossterm::terminal::disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    Ok(selection.map(|i| &inputs[i]))
}

fn run_app<'a, B: Backend>(
    terminal: &mut Terminal<B>,
    query: &str,
    mut list: StatefulList<Text<'a>>,
    tick_rate: Duration,
) -> Result<Option<usize>> {
    let mut last_tick = Instant::now();
    loop {
        terminal.draw(|f| ui(f, query, &mut list))?;

        let timeout = tick_rate
            .checked_sub(last_tick.elapsed())
            .unwrap_or_else(|| Duration::from_secs(0));

        if crossterm::event::poll(timeout)? {
            match event::read()? {
                Event::Key(key) => match key.code {
                    KeyCode::Char('q') => return Ok(None),
                    KeyCode::Esc => return Ok(None),
                    KeyCode::Down => list.next(),
                    KeyCode::Up => list.previous(),
                    KeyCode::PageDown => {
                        for _ in 0..10 {
                            list.next();
                        }
                    }
                    KeyCode::PageUp => {
                        for _ in 0..10 {
                            list.previous();
                        }
                    }
                    KeyCode::Enter => return Ok(list.selected()),
                    _ => {}
                },
                Event::Mouse(evt) => match evt.kind {
                    event::MouseEventKind::ScrollDown => list.next(),
                    event::MouseEventKind::ScrollUp => list.previous(),
                    _ => {}
                },
                _ => {}
            }
        }

        if last_tick.elapsed() >= tick_rate {
            last_tick = Instant::now();
        }
    }
}

fn ui<'a, B: Backend>(f: &mut Frame<B>, query: &str, list: &mut StatefulList<Text<'a>>) {
    let chunks = Layout::default()
        .margin(1)
        .direction(tui::layout::Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Min(1),
        ])
        .split(f.size());

    let items = list
        .items()
        .iter()
        .map(|i| ListItem::new(i.clone()))
        .collect_vec();

    let items = List::new(items)
        // .block(Block::default())
        .highlight_style(Style::default().add_modifier(Modifier::BOLD))
        .highlight_symbol("｜")
        .repeat_highlight_symbol(true);

    let search = Paragraph::new(Text::from(Spans::from(vec![
        Span::styled(
            "Search: ",
            Style::default()
                .fg(Color::Rgb(32, 178, 170))
                .add_modifier(Modifier::DIM),
        ),
        Span::styled(
            query,
            Style::default()
                .fg(Color::Rgb(73, 159, 147))
                .add_modifier(Modifier::BOLD),
        ),
    ])));

    f.render_widget(
        search,
        chunks[0].inner(&Margin {
            vertical: 0,
            horizontal: 2,
        }),
    );

    f.render_stateful_widget(items, chunks[2], list.state());
}

mod statefullist {
    use tui::widgets::ListState;

    pub struct StatefulList<T> {
        state: ListState,
        items: Vec<T>,
    }

    impl<T> StatefulList<T> {
        pub fn with_items(items: Vec<T>) -> StatefulList<T> {
            StatefulList {
                state: ListState::default(),
                items,
            }
        }

        pub fn items(&self) -> &[T] {
            &self.items
        }

        pub fn selected(&self) -> Option<usize> {
            self.state.selected()
        }

        pub fn next(&mut self) {
            let i = match self.state.selected() {
                Some(i) => {
                    if i >= self.items.len() - 1 {
                        0
                    } else {
                        i + 1
                    }
                }
                None => 0,
            };
            self.state.select(Some(i));
        }

        pub fn previous(&mut self) {
            let i = match self.state.selected() {
                Some(i) => {
                    if i == 0 {
                        self.items.len() - 1
                    } else {
                        i - 1
                    }
                }
                None => 0,
            };
            self.state.select(Some(i));
        }

        pub fn state(&mut self) -> &mut ListState {
            &mut self.state
        }
    }
}
