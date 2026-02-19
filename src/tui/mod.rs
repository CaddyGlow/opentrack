pub mod app;

use std::io;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crossterm::cursor;
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap};
use tokio::sync::mpsc;

use crate::Result;
use crate::cache::store;
use crate::config;
use crate::config::ParcelEntry;
use crate::providers::{self, ProviderRegistry};
use crate::tracking::{TrackOptions, TrackingInfo, TrackingStatus};
use app::{AddField, App, AppMode, FlashKind, LogLevel};

const PROVIDER_CHOICES: [&str; 2] = ["mondial-relay", "laposte"];

enum UiMessage {
    RefreshDone {
        index: usize,
        result: std::result::Result<TrackingInfo, String>,
    },
}

struct TerminalGuard;

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let mut stdout = io::stdout();
        let _ = execute!(stdout, cursor::Show, LeaveAlternateScreen);
    }
}

pub async fn run() -> Result<()> {
    let mut config = config::load().await?;
    let mut app = App::new(config.parcels.clone());
    load_cached_parcels(&mut app).await?;
    let http_client = providers::build_http_client(&config)?;
    let registry = Arc::new(ProviderRegistry::new(http_client, &config));
    let result = async {
        enable_raw_mode()?;
        let mut stdout = io::stdout();
        execute!(stdout, EnterAlternateScreen, cursor::Hide)?;
        let _guard = TerminalGuard;

        let backend = CrosstermBackend::new(stdout);
        let mut terminal = Terminal::new(backend)?;

        let (tx, mut rx) = mpsc::unbounded_channel::<UiMessage>();
        if !app.is_empty() {
            spawn_refresh_all(&mut app, &registry, &tx);
        }

        let refresh_interval = Duration::from_secs(config.general.watch_interval.max(1));
        let mut next_auto_refresh = Instant::now() + refresh_interval;

        loop {
            while let Ok(message) = rx.try_recv() {
                match message {
                    UiMessage::RefreshDone { index, result } => match result {
                        Ok(info) => {
                            let status = info.status.to_string();
                            let latest = info
                                .events
                                .first()
                                .map(|event| event.description.clone())
                                .unwrap_or_else(|| "-".to_string());
                            app.apply_refresh_success(index, info);
                            app.push_log_for_index(
                                index,
                                LogLevel::Info,
                                format!("Refresh succeeded (status={status}, latest=\"{latest}\")"),
                            );
                        }
                        Err(err) => {
                            app.apply_refresh_error(index, err.clone());
                            app.push_log_for_index(
                                index,
                                LogLevel::Error,
                                format!("Refresh failed: {err}"),
                            );
                        }
                    },
                }
            }

            if !app.is_empty() && Instant::now() >= next_auto_refresh {
                spawn_refresh_all(&mut app, &registry, &tx);
                next_auto_refresh = Instant::now() + refresh_interval;
            }

            terminal.draw(|frame| draw(frame, &app))?;

            if event::poll(Duration::from_millis(120))? {
                let input = event::read()?;
                if !handle_event(&mut app, &mut config, &registry, &tx, input).await {
                    break;
                }
            }

            app.tick_spinner();
        }

        Ok(())
    }
    .await;

    registry.shutdown().await;
    result
}

async fn load_cached_parcels(app: &mut App) -> Result<()> {
    for parcel in &mut app.parcels {
        match store::read(&parcel.entry.provider, &parcel.entry.id).await {
            Ok(Some(entry)) => {
                parcel.info = Some(entry.info);
                parcel.error = None;
            }
            Ok(None) => {}
            Err(err) => {
                parcel.error = Some(err.to_string());
            }
        }
    }
    Ok(())
}

async fn handle_event(
    app: &mut App,
    config: &mut config::Config,
    registry: &Arc<ProviderRegistry>,
    tx: &mpsc::UnboundedSender<UiMessage>,
    event: Event,
) -> bool {
    let Event::Key(key) = event else {
        return true;
    };

    if key.kind != KeyEventKind::Press {
        return true;
    }

    match app.mode {
        AppMode::Help => {
            match key.code {
                KeyCode::Esc | KeyCode::Char('?') | KeyCode::Char('q') => app.toggle_help(),
                _ => {}
            }
            return true;
        }
        AppMode::Add => {
            handle_add_mode_event(app, config, registry, tx, key).await;
            return true;
        }
        AppMode::ConfirmRemove => {
            handle_remove_mode_event(app, config, key).await;
            return true;
        }
        AppMode::Normal => {}
    }

    match key.code {
        KeyCode::Char('q') | KeyCode::Esc => return false,
        KeyCode::Char('?') => app.toggle_help(),
        KeyCode::Char('j') | KeyCode::Down => app.next_parcel(),
        KeyCode::Char('k') | KeyCode::Up => app.previous_parcel(),
        KeyCode::Char('J') | KeyCode::PageDown => app.scroll_events_down(),
        KeyCode::Char('K') | KeyCode::PageUp => app.scroll_events_up(),
        KeyCode::Char('r') => spawn_refresh_selected(app, registry, tx),
        KeyCode::Char('R') => spawn_refresh_all(app, registry, tx),
        KeyCode::Char('a') => {
            app.clear_flash();
            app.open_add_form();
        }
        KeyCode::Char('l') => {
            if app.is_empty() {
                app.set_flash(FlashKind::Info, "No tracking selected");
            } else if let Some(visible) = app.toggle_logs_for_selected() {
                let level = app.selected_log_level().unwrap_or(LogLevel::Info);
                let state = if visible { "shown" } else { "hidden" };
                app.set_flash(
                    FlashKind::Info,
                    format!("Logs {state} (filter={}+)", level.as_str()),
                );
            }
        }
        KeyCode::Char(']') => {
            if let Some(level) = app.increase_selected_log_level() {
                app.set_flash(
                    FlashKind::Info,
                    format!("Log filter set to {}+", level.as_str()),
                );
            } else {
                app.set_flash(FlashKind::Info, "No tracking selected");
            }
        }
        KeyCode::Char('[') => {
            if let Some(level) = app.decrease_selected_log_level() {
                app.set_flash(
                    FlashKind::Info,
                    format!("Log filter set to {}+", level.as_str()),
                );
            } else {
                app.set_flash(FlashKind::Info, "No tracking selected");
            }
        }
        KeyCode::Char('d') | KeyCode::Delete => {
            if app.is_empty() {
                app.set_flash(FlashKind::Info, "No tracking entry to remove");
            } else {
                app.clear_flash();
                app.open_remove_confirm();
            }
        }
        _ => {}
    }

    true
}

async fn handle_add_mode_event(
    app: &mut App,
    config: &mut config::Config,
    registry: &Arc<ProviderRegistry>,
    tx: &mpsc::UnboundedSender<UiMessage>,
    key: KeyEvent,
) {
    match key.code {
        KeyCode::Esc => {
            app.close_modal();
            app.set_flash(FlashKind::Info, "Add tracking canceled");
        }
        KeyCode::Tab | KeyCode::Down => app.add_form.focus_next(),
        KeyCode::BackTab | KeyCode::Up => app.add_form.focus_previous(),
        KeyCode::Right => {
            if app.add_form.focused == AddField::Provider {
                cycle_provider_choice(&mut app.add_form.provider, true);
            } else {
                app.add_form.focus_next();
            }
        }
        KeyCode::Left => {
            if app.add_form.focused == AddField::Provider {
                cycle_provider_choice(&mut app.add_form.provider, false);
            } else {
                app.add_form.focus_previous();
            }
        }
        KeyCode::Backspace => app.add_form.backspace(),
        KeyCode::Enter => {
            submit_add_form(app, config, registry, tx).await;
        }
        KeyCode::Char(c)
            if !key
                .modifiers
                .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
        {
            app.add_form.insert_char(c);
        }
        _ => {}
    }
}

async fn submit_add_form(
    app: &mut App,
    config: &mut config::Config,
    registry: &Arc<ProviderRegistry>,
    tx: &mpsc::UnboundedSender<UiMessage>,
) {
    let form = app.add_form.clone();
    let parcel_id = form.id.trim();
    let provider_id = form.provider.trim();

    if parcel_id.is_empty() {
        app.set_flash(FlashKind::Error, "Parcel ID is required");
        app.push_log_for_selected(LogLevel::Error, "Add tracking failed: missing parcel ID");
        return;
    }

    if provider_id.is_empty() {
        app.set_flash(FlashKind::Error, "Provider is required");
        app.push_log_for_selected(LogLevel::Error, "Add tracking failed: missing provider");
        return;
    }

    if config
        .parcels
        .iter()
        .any(|parcel| parcel.id == parcel_id && parcel.provider == provider_id)
    {
        app.set_flash(FlashKind::Error, "Tracking entry already exists");
        app.push_log_for_selected(
            LogLevel::Warn,
            format!("Add tracking skipped: {provider_id}/{parcel_id} already exists"),
        );
        return;
    }

    let notify = match parse_optional_bool(&form.notify) {
        Ok(notify) => notify,
        Err(err) => {
            app.set_flash(FlashKind::Error, err);
            app.push_log_for_selected(LogLevel::Error, "Add tracking failed: invalid notify field");
            return;
        }
    };

    if let Err(err) = registry.get_by_id(provider_id) {
        app.set_flash(FlashKind::Error, format!("Unknown provider: {err}"));
        app.push_log_for_selected(
            LogLevel::Error,
            format!("Add tracking failed: unknown provider '{provider_id}'"),
        );
        return;
    }

    let new_entry = ParcelEntry {
        id: parcel_id.to_string(),
        provider: provider_id.to_string(),
        label: to_optional_string(&form.label),
        postcode: to_optional_string(&form.postcode),
        lang: to_optional_string(&form.lang),
        notify,
    };

    config.parcels.push(new_entry.clone());
    if let Err(err) = config::save(config).await {
        config.parcels.pop();
        app.set_flash(FlashKind::Error, format!("Failed to save config: {err}"));
        app.push_log_for_selected(
            LogLevel::Error,
            format!("Add tracking failed: unable to save config ({err})"),
        );
        return;
    }

    app.push_parcel(new_entry.clone());
    app.push_log_for_selected(
        LogLevel::Info,
        format!(
            "Tracking added via TUI: {}/{}",
            new_entry.provider, new_entry.id
        ),
    );
    app.close_modal();
    app.set_flash(
        FlashKind::Success,
        format!("Added tracking {}/{}", new_entry.provider, new_entry.id),
    );
    spawn_refresh_selected(app, registry, tx);
}

async fn handle_remove_mode_event(app: &mut App, config: &mut config::Config, key: KeyEvent) {
    match key.code {
        KeyCode::Esc | KeyCode::Char('n') | KeyCode::Char('N') => {
            app.close_modal();
            app.set_flash(FlashKind::Info, "Remove canceled");
        }
        KeyCode::Char('y') | KeyCode::Char('Y') | KeyCode::Enter => {
            remove_selected_tracking(app, config).await;
        }
        _ => {}
    }
}

async fn remove_selected_tracking(app: &mut App, config: &mut config::Config) {
    let Some((target_id, target_provider)) = app
        .selected_parcel()
        .map(|parcel| (parcel.entry.id.clone(), parcel.entry.provider.clone()))
    else {
        app.close_modal();
        app.set_flash(FlashKind::Info, "No tracking entry selected");
        return;
    };

    let previous = config.parcels.clone();
    config
        .parcels
        .retain(|parcel| !(parcel.id == target_id && parcel.provider == target_provider));

    if config.parcels.len() == previous.len() {
        app.close_modal();
        app.set_flash(
            FlashKind::Error,
            "Selected tracking entry not found in config",
        );
        app.push_log_for_selected(
            LogLevel::Error,
            format!("Remove failed: {target_provider}/{target_id} not found in config"),
        );
        return;
    }

    if let Err(err) = config::save(config).await {
        config.parcels = previous;
        app.close_modal();
        app.set_flash(FlashKind::Error, format!("Failed to save config: {err}"));
        app.push_log_for_selected(
            LogLevel::Error,
            format!("Remove failed: unable to save config ({err})"),
        );
        return;
    }

    app.remove_selected();
    app.close_modal();
    app.set_flash(
        FlashKind::Success,
        format!("Removed tracking {}/{}", target_provider, target_id),
    );
    app.push_log_for_selected(
        LogLevel::Info,
        format!("Tracking removed via TUI: {target_provider}/{target_id}"),
    );
}

fn to_optional_string(raw: &str) -> Option<String> {
    let value = raw.trim();
    (!value.is_empty()).then(|| value.to_string())
}

fn parse_optional_bool(raw: &str) -> std::result::Result<Option<bool>, String> {
    let value = raw.trim();
    if value.is_empty() {
        return Ok(None);
    }

    match value.to_ascii_lowercase().as_str() {
        "true" | "1" | "yes" | "y" => Ok(Some(true)),
        "false" | "0" | "no" | "n" => Ok(Some(false)),
        _ => Err("Notify must be empty or one of: true/false/yes/no/1/0".to_string()),
    }
}

fn cycle_provider_choice(provider: &mut String, next: bool) {
    let current = provider.trim();
    let current_index = PROVIDER_CHOICES
        .iter()
        .position(|candidate| candidate.eq_ignore_ascii_case(current))
        .unwrap_or(0);

    let next_index = if next {
        (current_index + 1) % PROVIDER_CHOICES.len()
    } else {
        (current_index + PROVIDER_CHOICES.len() - 1) % PROVIDER_CHOICES.len()
    };

    *provider = PROVIDER_CHOICES[next_index].to_string();
}

fn spawn_refresh_selected(
    app: &mut App,
    registry: &Arc<ProviderRegistry>,
    tx: &mpsc::UnboundedSender<UiMessage>,
) {
    if app.is_empty() {
        return;
    }

    let index = app.selected;
    spawn_refresh_for_index(app, registry, tx, index);
}

fn spawn_refresh_all(
    app: &mut App,
    registry: &Arc<ProviderRegistry>,
    tx: &mpsc::UnboundedSender<UiMessage>,
) {
    for index in 0..app.parcels.len() {
        spawn_refresh_for_index(app, registry, tx, index);
    }
}

fn spawn_refresh_for_index(
    app: &mut App,
    registry: &Arc<ProviderRegistry>,
    tx: &mpsc::UnboundedSender<UiMessage>,
    index: usize,
) {
    let Some(entry) = app.parcels.get(index).map(|parcel| parcel.entry.clone()) else {
        return;
    };

    app.push_log_for_index(index, LogLevel::Info, "Refresh started");
    app.set_fetching(index, true);

    let tx = tx.clone();
    let registry = Arc::clone(registry);
    tokio::spawn(async move {
        let result = refresh_parcel(entry, registry)
            .await
            .map_err(|err| err.to_string());
        let _ = tx.send(UiMessage::RefreshDone { index, result });
    });
}

async fn refresh_parcel(
    entry: ParcelEntry,
    registry: Arc<ProviderRegistry>,
) -> Result<TrackingInfo> {
    let provider = registry.get_by_id(&entry.provider)?;

    let options = TrackOptions {
        postcode: entry.postcode.clone(),
        lang: entry.lang.clone(),
        no_cache: true,
    };

    let info = provider.track(&entry.id, &options).await?;
    store::write(provider.id(), &entry.id, &info).await?;
    Ok(info)
}

fn draw(frame: &mut ratatui::Frame<'_>, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(5),
            Constraint::Min(7),
            Constraint::Length(6),
        ])
        .split(frame.area());

    draw_top(frame, chunks[0], app);

    let main = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(35), Constraint::Percentage(65)])
        .split(chunks[1]);

    draw_parcels(frame, main[0], app);
    draw_right_panel(frame, main[1], app);
    draw_footer(frame, chunks[2], app);

    match app.mode {
        AppMode::Help => draw_help_overlay(frame),
        AppMode::Add => draw_add_overlay(frame, app),
        AppMode::ConfirmRemove => draw_remove_overlay(frame, app),
        AppMode::Normal => {}
    }
}

fn draw_right_panel(frame: &mut ratatui::Frame<'_>, area: Rect, app: &App) {
    if app.selected_logs_visible() {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Percentage(65), Constraint::Percentage(35)])
            .split(area);
        draw_events(frame, chunks[0], app);
        draw_logs(frame, chunks[1], app);
    } else {
        draw_events(frame, area, app);
    }
}

fn draw_top(frame: &mut ratatui::Frame<'_>, area: Rect, app: &App) {
    let mut lines = vec![
        Line::from(vec![
            Span::styled(
                " opentrack ",
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" "),
            Span::styled(
                "Parcel Tracking Dashboard",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(vec![
            Span::styled("[a]", Style::default().fg(Color::Green)),
            Span::raw(" add  "),
            Span::styled("[d]", Style::default().fg(Color::Red)),
            Span::raw(" remove  "),
            Span::styled("[l]", Style::default().fg(Color::Magenta)),
            Span::raw(" logs  "),
            Span::styled("[[]/[]]", Style::default().fg(Color::DarkGray)),
            Span::raw(" level  "),
            Span::styled("[r]", Style::default().fg(Color::Yellow)),
            Span::raw(" refresh  "),
            Span::styled("[R]", Style::default().fg(Color::Yellow)),
            Span::raw(" refresh all  "),
            Span::styled("[?]", Style::default().fg(Color::Blue)),
            Span::raw(" help  "),
            Span::styled("[q]", Style::default().fg(Color::Magenta)),
            Span::raw(" quit"),
        ]),
    ];

    if let Some(flash) = &app.flash {
        lines.push(Line::from(Span::styled(
            flash.text.as_str(),
            flash_style(flash.kind),
        )));
    } else {
        lines.push(Line::from(Span::styled(
            "Use j/k to switch parcels, J/K to scroll events",
            Style::default().fg(Color::DarkGray),
        )));
    }

    let top = Paragraph::new(lines).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Cyan)),
    );
    frame.render_widget(top, area);
}

fn draw_parcels(frame: &mut ratatui::Frame<'_>, area: Rect, app: &App) {
    let spinner = app.spinner_char();

    let items: Vec<ListItem<'_>> = if app.parcels.is_empty() {
        vec![ListItem::new(Line::from(Span::styled(
            "No parcels configured. Press 'a' to add tracking.",
            Style::default().fg(Color::DarkGray),
        )))]
    } else {
        app.parcels
            .iter()
            .map(|parcel| {
                let label = parcel.entry.label.as_deref().unwrap_or("-");
                let status_style = status_style(parcel.info.as_ref().map(|info| &info.status));

                let mut spans = vec![
                    Span::styled(parcel.status_tag(), status_style),
                    Span::raw(" "),
                    Span::styled(
                        &parcel.entry.id,
                        Style::default().add_modifier(Modifier::BOLD),
                    ),
                    Span::raw(" "),
                    Span::styled(
                        format!("[{}]", parcel.entry.provider),
                        Style::default().fg(Color::DarkGray),
                    ),
                ];

                if parcel.fetching {
                    spans.push(Span::raw(" "));
                    spans.push(Span::styled(
                        format!("{spinner}"),
                        Style::default()
                            .fg(Color::Cyan)
                            .add_modifier(Modifier::BOLD),
                    ));
                }

                if parcel.error.is_some() {
                    spans.push(Span::raw(" "));
                    spans.push(Span::styled(
                        "!",
                        Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
                    ));
                }

                spans.push(Span::raw("  "));
                spans.push(Span::styled(label, Style::default().fg(Color::Gray)));

                ListItem::new(Line::from(spans))
            })
            .collect()
    };

    let title = format!("PARCELS ({})", app.parcels.len());
    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(title)
                .border_style(Style::default().fg(Color::Blue)),
        )
        .highlight_style(
            Style::default()
                .bg(Color::Blue)
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol(" > ");

    let mut state = ListState::default();
    if !app.parcels.is_empty() {
        state.select(Some(app.selected));
    }
    frame.render_stateful_widget(list, area, &mut state);
}

fn status_style(status: Option<&TrackingStatus>) -> Style {
    match status {
        Some(TrackingStatus::Delivered) => Style::default().fg(Color::Green),
        Some(TrackingStatus::InTransit) | Some(TrackingStatus::OutForDelivery) => {
            Style::default().fg(Color::Yellow)
        }
        Some(TrackingStatus::Exception) => Style::default().fg(Color::Red),
        Some(TrackingStatus::PreShipment) => Style::default().fg(Color::Blue),
        Some(TrackingStatus::Unknown) => Style::default().fg(Color::DarkGray),
        None => Style::default().fg(Color::Gray),
    }
}

fn flash_style(kind: FlashKind) -> Style {
    match kind {
        FlashKind::Info => Style::default().fg(Color::Cyan),
        FlashKind::Success => Style::default().fg(Color::Green),
        FlashKind::Error => Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
    }
}

fn log_level_style(level: LogLevel) -> Style {
    match level {
        LogLevel::Error => Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        LogLevel::Warn => Style::default().fg(Color::Yellow),
        LogLevel::Info => Style::default().fg(Color::Cyan),
        LogLevel::Debug => Style::default().fg(Color::DarkGray),
    }
}

fn draw_events(frame: &mut ratatui::Frame<'_>, area: Rect, app: &App) {
    let block = Block::default()
        .borders(Borders::ALL)
        .title("EVENTS")
        .border_style(Style::default().fg(Color::Blue));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let Some(parcel) = app.selected_parcel() else {
        frame.render_widget(Paragraph::new("No parcel selected"), inner);
        return;
    };

    let Some(info) = &parcel.info else {
        let fallback = parcel
            .error
            .as_deref()
            .map(|err| format!("Error: {err}"))
            .unwrap_or_else(|| "No tracking data yet".to_string());
        frame.render_widget(
            Paragraph::new(fallback).style(Style::default().fg(Color::DarkGray)),
            inner,
        );
        return;
    };

    let visible_rows = inner.height as usize;
    let items: Vec<ListItem<'_>> = info
        .events
        .iter()
        .skip(app.event_scroll)
        .take(visible_rows)
        .map(|event| {
            let ts = event.timestamp.format("%Y-%m-%d %H:%M").to_string();
            let mut spans = vec![
                Span::styled(ts, Style::default().fg(Color::Cyan)),
                Span::raw("  "),
                Span::styled(
                    event.description.clone(),
                    Style::default().add_modifier(Modifier::BOLD),
                ),
            ];

            if let Some(location) = event.location.as_deref().filter(|value| !value.is_empty()) {
                spans.push(Span::raw("  "));
                spans.push(Span::styled(
                    format!("({location})"),
                    Style::default().fg(Color::Yellow),
                ));
            }

            ListItem::new(Line::from(spans))
        })
        .collect();

    if items.is_empty() {
        frame.render_widget(
            Paragraph::new("No events available").style(Style::default().fg(Color::DarkGray)),
            inner,
        );
    } else {
        frame.render_widget(List::new(items), inner);
    }
}

fn draw_footer(frame: &mut ratatui::Frame<'_>, area: Rect, app: &App) {
    let mut lines = Vec::new();

    if let Some(parcel) = app.selected_parcel() {
        let label = parcel.entry.label.as_deref().unwrap_or("-");
        lines.push(Line::from(vec![
            Span::styled("Provider: ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                parcel.entry.provider.as_str(),
                Style::default().add_modifier(Modifier::BOLD),
            ),
            Span::raw("    "),
            Span::styled("ID: ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                parcel.entry.id.as_str(),
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
        ]));
        lines.push(Line::from(vec![
            Span::styled("Label: ", Style::default().fg(Color::DarkGray)),
            Span::styled(label, Style::default().fg(Color::Gray)),
        ]));

        match &parcel.info {
            Some(info) => {
                let estimated = info
                    .estimated_delivery
                    .map(|value| value.format("%Y-%m-%d %H:%M").to_string())
                    .unwrap_or_else(|| "-".to_string());
                lines.push(Line::from(vec![
                    Span::styled("Status: ", Style::default().fg(Color::DarkGray)),
                    Span::styled(info.status.to_string(), status_style(Some(&info.status))),
                    Span::raw("    "),
                    Span::styled("ETA: ", Style::default().fg(Color::DarkGray)),
                    Span::styled(estimated, Style::default().fg(Color::Yellow)),
                ]));
            }
            None => {
                let message = parcel
                    .error
                    .as_ref()
                    .map(|err| format!("Error: {err}"))
                    .unwrap_or_else(|| "Status: waiting for first refresh".to_string());
                lines.push(Line::from(Span::styled(
                    message,
                    Style::default().fg(Color::DarkGray),
                )));
            }
        }
    } else {
        lines.push(Line::from("No parcel selected"));
        lines.push(Line::from(""));
        lines.push(Line::from(""));
    }

    if let Some(flash) = &app.flash {
        lines.push(Line::from(Span::styled(
            flash.text.as_str(),
            flash_style(flash.kind),
        )));
    }

    let details = Paragraph::new(lines).wrap(Wrap { trim: true }).block(
        Block::default()
            .borders(Borders::ALL)
            .title("ESSENTIAL")
            .border_style(Style::default().fg(Color::Blue)),
    );
    frame.render_widget(details, area);
}

fn draw_logs(frame: &mut ratatui::Frame<'_>, area: Rect, app: &App) {
    let title = app
        .selected_parcel()
        .map(|parcel| format!("LOGS [{}+]", parcel.min_log_level.as_str()))
        .unwrap_or_else(|| "LOGS".to_string());

    let block = Block::default()
        .borders(Borders::ALL)
        .title(title)
        .border_style(Style::default().fg(Color::Magenta));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let Some(parcel) = app.selected_parcel() else {
        frame.render_widget(
            Paragraph::new("No parcel selected").style(Style::default().fg(Color::DarkGray)),
            inner,
        );
        return;
    };

    let visible_rows = inner.height as usize;
    let filtered: Vec<_> = parcel
        .logs
        .iter()
        .filter(|entry| parcel.min_log_level.allows(entry.level))
        .collect();
    let start = filtered.len().saturating_sub(visible_rows);

    let rows: Vec<ListItem<'_>> = filtered
        .into_iter()
        .skip(start)
        .map(|entry| {
            let ts = entry.timestamp.format("%H:%M:%S").to_string();
            ListItem::new(Line::from(vec![
                Span::styled(ts, Style::default().fg(Color::DarkGray)),
                Span::raw(" "),
                Span::styled(
                    entry.level.as_str().to_string(),
                    log_level_style(entry.level),
                ),
                Span::raw(" "),
                Span::raw(entry.message.clone()),
            ]))
        })
        .collect();

    if rows.is_empty() {
        frame.render_widget(
            Paragraph::new("No log entries at current filter")
                .style(Style::default().fg(Color::DarkGray)),
            inner,
        );
    } else {
        frame.render_widget(List::new(rows), inner);
    }
}

fn draw_help_overlay(frame: &mut ratatui::Frame<'_>) {
    let area = centered_rect(74, 70, frame.area());
    frame.render_widget(Clear, area);

    let help = Paragraph::new(
        "j / Down: Next parcel\n\
k / Up: Previous parcel\n\
J / PageDown: Scroll events down\n\
K / PageUp: Scroll events up\n\
r: Refresh selected parcel\n\
R: Refresh all parcels\n\
a: Add tracking entry\n\
d / Delete: Remove selected tracking\n\
l: Show/hide logs for selected tracking\n\
[ / ]: Decrease/increase log filter level\n\
q / Esc: Quit\n\
?: Toggle help",
    )
    .block(
        Block::default()
            .title("Help")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Cyan)),
    )
    .alignment(Alignment::Left);

    frame.render_widget(help, area);
}

fn draw_add_overlay(frame: &mut ratatui::Frame<'_>, app: &App) {
    let area = centered_rect(78, 78, frame.area());
    frame.render_widget(Clear, area);

    let form = &app.add_form;
    let rows = vec![
        add_form_row("Parcel ID", &form.id, form.focused == AddField::Id, true),
        add_form_row(
            "Provider",
            &form.provider,
            form.focused == AddField::Provider,
            true,
        ),
        add_form_row("Label", &form.label, form.focused == AddField::Label, false),
        add_form_row(
            "Postcode",
            &form.postcode,
            form.focused == AddField::Postcode,
            false,
        ),
        add_form_row("Lang", &form.lang, form.focused == AddField::Lang, false),
        add_form_row(
            "Notify (true/false)",
            &form.notify,
            form.focused == AddField::Notify,
            false,
        ),
    ];

    let mut lines = vec![
        Line::from(Span::styled(
            "Tab/Up/Down: move field   Enter: save   Esc: cancel",
            Style::default().fg(Color::DarkGray),
        )),
        Line::from(Span::styled(
            "Left/Right on Provider: cycle choices",
            Style::default().fg(Color::DarkGray),
        )),
        provider_choices_line(&form.provider),
        Line::from(""),
    ];
    lines.extend(rows);

    let modal = Paragraph::new(lines)
        .block(
            Block::default()
                .title("Add Tracking")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Green)),
        )
        .alignment(Alignment::Left);

    frame.render_widget(modal, area);
}

fn provider_choices_line(current: &str) -> Line<'static> {
    let current = current.trim();
    let mut spans = vec![Span::styled(
        "Providers: ",
        Style::default().fg(Color::DarkGray),
    )];

    for (index, candidate) in PROVIDER_CHOICES.iter().enumerate() {
        if index > 0 {
            spans.push(Span::raw("  "));
        }

        let style = if candidate.eq_ignore_ascii_case(current) {
            Style::default()
                .fg(Color::Black)
                .bg(Color::Cyan)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::Gray)
        };
        spans.push(Span::styled((*candidate).to_string(), style));
    }

    Line::from(spans)
}

fn add_form_row(name: &str, value: &str, focused: bool, required: bool) -> Line<'static> {
    let marker = if focused { ">" } else { " " };
    let suffix = if required { "*" } else { "" };
    let shown = if value.is_empty() { "<empty>" } else { value };

    let value_style = if focused {
        Style::default()
            .fg(Color::Black)
            .bg(Color::Yellow)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::White)
    };

    Line::from(vec![
        Span::styled(
            format!("{marker} {name}{suffix}: "),
            Style::default().fg(Color::DarkGray),
        ),
        Span::styled(shown.to_string(), value_style),
    ])
}

fn draw_remove_overlay(frame: &mut ratatui::Frame<'_>, app: &App) {
    let area = centered_rect(60, 34, frame.area());
    frame.render_widget(Clear, area);

    let target = app
        .selected_parcel()
        .map(|parcel| format!("{}/{}", parcel.entry.provider, parcel.entry.id))
        .unwrap_or_else(|| "-".to_string());

    let modal = Paragraph::new(vec![
        Line::from(vec![
            Span::styled("Remove tracking: ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                target,
                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(""),
        Line::from("Press y or Enter to confirm removal."),
        Line::from("Press n or Esc to cancel."),
    ])
    .block(
        Block::default()
            .title("Confirm Remove")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Red)),
    )
    .alignment(Alignment::Left);

    frame.render_widget(modal, area);
}

fn centered_rect(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(area);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(vertical[1])[1]
}
