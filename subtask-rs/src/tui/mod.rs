//! TUI module - Interactive terminal user interface
//!
//! Provides a real-time view of tasks and their status using ratatui.

use std::io;
use std::time::Duration;

use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Cell, Paragraph, Row, Table, TableState},
    Frame, Terminal,
};

use crate::task::{list_tasks, Task, TaskStatus, WorkerStatus};

/// Application state for the TUI
pub struct App {
    /// All tasks
    tasks: Vec<Task>,

    /// Table selection state
    table_state: TableState,

    /// Whether to quit
    should_quit: bool,

    /// Error message to display
    error: Option<String>,
}

impl App {
    /// Creates a new App instance
    pub fn new() -> anyhow::Result<Self> {
        let tasks = Self::load_tasks()?;
        let mut table_state = TableState::default();
        if !tasks.is_empty() {
            table_state.select(Some(0));
        }

        Ok(App {
            tasks,
            table_state,
            should_quit: false,
            error: None,
        })
    }

    /// Loads all tasks
    fn load_tasks() -> anyhow::Result<Vec<Task>> {
        let names = list_tasks()?;
        let mut tasks = Vec::new();

        for name in names {
            match Task::load(&name) {
                Ok(task) => tasks.push(task),
                Err(e) => {
                    tracing::warn!("could not load task {}: {}", name, e);
                }
            }
        }

        // Sort by updated_at descending
        tasks.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));

        Ok(tasks)
    }

    /// Refreshes the task list
    fn refresh(&mut self) {
        match Self::load_tasks() {
            Ok(tasks) => {
                self.tasks = tasks;
                self.error = None;
            }
            Err(e) => {
                self.error = Some(format!("Failed to refresh: {}", e));
            }
        }
    }

    /// Moves selection up
    fn previous(&mut self) {
        if self.tasks.is_empty() {
            return;
        }
        let i = match self.table_state.selected() {
            Some(i) => {
                if i == 0 {
                    self.tasks.len() - 1
                } else {
                    i - 1
                }
            }
            None => 0,
        };
        self.table_state.select(Some(i));
    }

    /// Moves selection down
    fn next(&mut self) {
        if self.tasks.is_empty() {
            return;
        }
        let i = match self.table_state.selected() {
            Some(i) => {
                if i >= self.tasks.len() - 1 {
                    0
                } else {
                    i + 1
                }
            }
            None => 0,
        };
        self.table_state.select(Some(i));
    }

    /// Gets the selected task
    fn selected_task(&self) -> Option<&Task> {
        self.table_state.selected().and_then(|i| self.tasks.get(i))
    }
}

/// Runs the TUI
pub fn run() -> anyhow::Result<()> {
    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Create app
    let mut app = App::new()?;

    // Run main loop
    let result = run_app(&mut terminal, &mut app);

    // Restore terminal
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    result
}

fn run_app<B: ratatui::backend::Backend>(
    terminal: &mut Terminal<B>,
    app: &mut App,
) -> anyhow::Result<()> {
    loop {
        terminal.draw(|f| ui(f, app))?;

        // Poll for events with timeout for auto-refresh
        if event::poll(Duration::from_secs(2))? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    match key.code {
                        KeyCode::Char('q') | KeyCode::Esc => {
                            app.should_quit = true;
                        }
                        KeyCode::Char('r') => {
                            app.refresh();
                        }
                        KeyCode::Up | KeyCode::Char('k') => {
                            app.previous();
                        }
                        KeyCode::Down | KeyCode::Char('j') => {
                            app.next();
                        }
                        _ => {}
                    }
                }
            }
        } else {
            // Auto-refresh on timeout
            app.refresh();
        }

        if app.should_quit {
            return Ok(());
        }
    }
}

fn ui(f: &mut Frame, app: &mut App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Header
            Constraint::Min(5),    // Task table
            Constraint::Length(8), // Detail panel
            Constraint::Length(1), // Footer
        ])
        .split(f.size());

    // Header
    let header = Paragraph::new("Subtask - Task Manager")
        .style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))
        .block(Block::default().borders(Borders::BOTTOM));
    f.render_widget(header, chunks[0]);

    // Task table
    render_task_table(f, app, chunks[1]);

    // Detail panel
    if let Some(task) = app.selected_task() {
        render_detail_panel(f, task, chunks[2]);
    } else {
        let empty = Paragraph::new("No task selected")
            .block(Block::default().borders(Borders::ALL).title("Details"));
        f.render_widget(empty, chunks[2]);
    }

    // Footer
    let footer_text = if let Some(ref error) = app.error {
        Span::styled(error, Style::default().fg(Color::Red))
    } else {
        Span::raw("↑/↓ Navigate | r Refresh | q Quit")
    };
    let footer = Paragraph::new(Line::from(footer_text));
    f.render_widget(footer, chunks[3]);
}

fn render_task_table(f: &mut Frame, app: &mut App, area: Rect) {
    let header_cells = ["Name", "Status", "Worker", "Stage", "Title"]
        .iter()
        .map(|h| Cell::from(*h).style(Style::default().add_modifier(Modifier::BOLD)));
    let header = Row::new(header_cells).height(1);

    let rows = app.tasks.iter().map(|task| {
        let status_style = match task.status {
            TaskStatus::Open => Style::default().fg(Color::Green),
            TaskStatus::Merged => Style::default().fg(Color::Blue),
            TaskStatus::Closed => Style::default().fg(Color::Gray),
        };

        let worker_style = match task.worker_status {
            WorkerStatus::Idle => Style::default().fg(Color::Gray),
            WorkerStatus::Running => Style::default().fg(Color::Yellow),
            WorkerStatus::Replied => Style::default().fg(Color::Green),
            WorkerStatus::Error => Style::default().fg(Color::Red),
        };

        let cells = vec![
            Cell::from(task.name.clone()),
            Cell::from(task.status.to_string()).style(status_style),
            Cell::from(task.worker_status.to_string()).style(worker_style),
            Cell::from(task.stage.to_string()),
            Cell::from(truncate(&task.title, 30)),
        ];
        Row::new(cells).height(1)
    });

    let table = Table::new(
        rows,
        [
            Constraint::Percentage(25),
            Constraint::Length(10),
            Constraint::Length(10),
            Constraint::Length(10),
            Constraint::Percentage(35),
        ],
    )
    .header(header)
    .block(Block::default().borders(Borders::ALL).title("Tasks"))
    .highlight_style(Style::default().add_modifier(Modifier::REVERSED));

    f.render_stateful_widget(table, area, &mut app.table_state);
}

fn render_detail_panel(f: &mut Frame, task: &Task, area: Rect) {
    let mut lines = vec![
        Line::from(vec![
            Span::styled("Task: ", Style::default().add_modifier(Modifier::BOLD)),
            Span::raw(&task.name),
        ]),
        Line::from(vec![
            Span::styled("Title: ", Style::default().add_modifier(Modifier::BOLD)),
            Span::raw(&task.title),
        ]),
        Line::from(vec![
            Span::styled("Branch: ", Style::default().add_modifier(Modifier::BOLD)),
            Span::raw(&task.name),
            Span::raw(" (from "),
            Span::raw(&task.base_branch),
            Span::raw(")"),
        ]),
    ];

    if let Some(ref ws) = task.workspace_path {
        lines.push(Line::from(vec![
            Span::styled("Workspace: ", Style::default().add_modifier(Modifier::BOLD)),
            Span::raw(ws.to_string_lossy().to_string()),
        ]));
    }

    if !task.description.is_empty() {
        lines.push(Line::from(""));
        lines.push(Line::from(truncate(&task.description, 80)));
    }

    let detail = Paragraph::new(lines)
        .block(Block::default().borders(Borders::ALL).title("Details"));
    f.render_widget(detail, area);
}

fn truncate(s: &str, max_len: usize) -> String {
    if s.len() > max_len {
        format!("{}...", &s[..max_len - 3])
    } else {
        s.to_string()
    }
}
