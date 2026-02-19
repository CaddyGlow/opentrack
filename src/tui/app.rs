use std::collections::VecDeque;

use chrono::{DateTime, Utc};

use crate::config::ParcelEntry;
use crate::tracking::{TrackingInfo, TrackingStatus};

const MAX_LOG_ENTRIES: usize = 250;

#[derive(Debug)]
pub struct App {
    pub parcels: Vec<ParcelState>,
    pub selected: usize,
    pub event_scroll: usize,
    pub mode: AppMode,
    pub add_form: AddForm,
    pub flash: Option<FlashMessage>,
    spinner_tick: usize,
}

#[derive(Debug)]
pub struct ParcelState {
    pub entry: ParcelEntry,
    pub info: Option<TrackingInfo>,
    pub fetching: bool,
    pub error: Option<String>,
    pub logs: VecDeque<LogEntry>,
    pub show_logs: bool,
    pub min_log_level: LogLevel,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum AppMode {
    #[default]
    Normal,
    Help,
    Add,
    ConfirmRemove,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlashKind {
    Info,
    Success,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogLevel {
    Error,
    Warn,
    Info,
    Debug,
}

impl Default for LogLevel {
    fn default() -> Self {
        Self::Info
    }
}

impl LogLevel {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Error => "ERROR",
            Self::Warn => "WARN",
            Self::Info => "INFO",
            Self::Debug => "DEBUG",
        }
    }

    pub fn increase(self) -> Self {
        match self {
            Self::Error => Self::Warn,
            Self::Warn => Self::Info,
            Self::Info => Self::Debug,
            Self::Debug => Self::Debug,
        }
    }

    pub fn decrease(self) -> Self {
        match self {
            Self::Error => Self::Error,
            Self::Warn => Self::Error,
            Self::Info => Self::Warn,
            Self::Debug => Self::Info,
        }
    }

    pub fn allows(self, entry_level: LogLevel) -> bool {
        entry_level.rank() <= self.rank()
    }

    fn rank(self) -> u8 {
        match self {
            Self::Error => 0,
            Self::Warn => 1,
            Self::Info => 2,
            Self::Debug => 3,
        }
    }
}

#[derive(Debug, Clone)]
pub struct LogEntry {
    pub timestamp: DateTime<Utc>,
    pub level: LogLevel,
    pub message: String,
}

#[derive(Debug, Clone)]
pub struct FlashMessage {
    pub kind: FlashKind,
    pub text: String,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum AddField {
    #[default]
    Id,
    Provider,
    Label,
    Postcode,
    Lang,
    Notify,
}

#[derive(Debug, Clone, Default)]
pub struct AddForm {
    pub id: String,
    pub provider: String,
    pub label: String,
    pub postcode: String,
    pub lang: String,
    pub notify: String,
    pub focused: AddField,
}

impl AddField {
    pub fn next(self) -> Self {
        match self {
            Self::Id => Self::Provider,
            Self::Provider => Self::Label,
            Self::Label => Self::Postcode,
            Self::Postcode => Self::Lang,
            Self::Lang => Self::Notify,
            Self::Notify => Self::Id,
        }
    }

    pub fn previous(self) -> Self {
        match self {
            Self::Id => Self::Notify,
            Self::Provider => Self::Id,
            Self::Label => Self::Provider,
            Self::Postcode => Self::Label,
            Self::Lang => Self::Postcode,
            Self::Notify => Self::Lang,
        }
    }
}

impl AddForm {
    pub fn reset(&mut self, provider_hint: Option<&str>) {
        self.id.clear();
        self.provider = provider_hint.unwrap_or("mondial-relay").to_string();
        self.label.clear();
        self.postcode.clear();
        self.lang.clear();
        self.notify.clear();
        self.focused = AddField::Id;
    }

    pub fn focus_next(&mut self) {
        self.focused = self.focused.next();
    }

    pub fn focus_previous(&mut self) {
        self.focused = self.focused.previous();
    }

    pub fn insert_char(&mut self, c: char) {
        self.current_field_mut().push(c);
    }

    pub fn backspace(&mut self) {
        self.current_field_mut().pop();
    }

    fn current_field_mut(&mut self) -> &mut String {
        match self.focused {
            AddField::Id => &mut self.id,
            AddField::Provider => &mut self.provider,
            AddField::Label => &mut self.label,
            AddField::Postcode => &mut self.postcode,
            AddField::Lang => &mut self.lang,
            AddField::Notify => &mut self.notify,
        }
    }
}

impl App {
    pub fn new(entries: Vec<ParcelEntry>) -> Self {
        let parcels = entries
            .into_iter()
            .map(|entry| ParcelState {
                entry,
                info: None,
                fetching: false,
                error: None,
                logs: VecDeque::new(),
                show_logs: false,
                min_log_level: LogLevel::Info,
            })
            .collect();

        let mut app = Self {
            parcels,
            selected: 0,
            event_scroll: 0,
            mode: AppMode::Normal,
            add_form: AddForm::default(),
            flash: None,
            spinner_tick: 0,
        };
        let provider_hint = app
            .selected_parcel()
            .map(|parcel| parcel.entry.provider.clone());
        app.add_form.reset(provider_hint.as_deref());
        app
    }

    pub fn is_empty(&self) -> bool {
        self.parcels.is_empty()
    }

    pub fn selected_parcel(&self) -> Option<&ParcelState> {
        self.parcels.get(self.selected)
    }

    pub fn selected_parcel_mut(&mut self) -> Option<&mut ParcelState> {
        self.parcels.get_mut(self.selected)
    }

    pub fn set_fetching(&mut self, index: usize, fetching: bool) {
        if let Some(parcel) = self.parcels.get_mut(index) {
            parcel.fetching = fetching;
            if fetching {
                parcel.error = None;
            }
        }
    }

    pub fn apply_refresh_success(&mut self, index: usize, info: TrackingInfo) {
        if let Some(parcel) = self.parcels.get_mut(index) {
            parcel.info = Some(info);
            parcel.fetching = false;
            parcel.error = None;
        }
    }

    pub fn apply_refresh_error(&mut self, index: usize, error: String) {
        if let Some(parcel) = self.parcels.get_mut(index) {
            parcel.fetching = false;
            parcel.error = Some(error);
        }
    }

    pub fn next_parcel(&mut self) {
        if self.parcels.is_empty() {
            return;
        }

        self.selected = (self.selected + 1).min(self.parcels.len() - 1);
        self.event_scroll = 0;
    }

    pub fn previous_parcel(&mut self) {
        if self.parcels.is_empty() {
            return;
        }

        self.selected = self.selected.saturating_sub(1);
        self.event_scroll = 0;
    }

    pub fn scroll_events_down(&mut self) {
        let Some(parcel) = self.selected_parcel() else {
            return;
        };

        let max_index = parcel
            .info
            .as_ref()
            .map_or(0, |info| info.events.len().saturating_sub(1));
        self.event_scroll = (self.event_scroll + 1).min(max_index);
    }

    pub fn scroll_events_up(&mut self) {
        self.event_scroll = self.event_scroll.saturating_sub(1);
    }

    pub fn toggle_help(&mut self) {
        self.mode = match self.mode {
            AppMode::Normal => AppMode::Help,
            AppMode::Help => AppMode::Normal,
            _ => AppMode::Normal,
        };
    }

    pub fn open_add_form(&mut self) {
        let provider_hint = self
            .selected_parcel()
            .map(|parcel| parcel.entry.provider.clone());
        self.add_form.reset(provider_hint.as_deref());
        self.mode = AppMode::Add;
    }

    pub fn open_remove_confirm(&mut self) {
        self.mode = AppMode::ConfirmRemove;
    }

    pub fn close_modal(&mut self) {
        self.mode = AppMode::Normal;
    }

    pub fn set_flash(&mut self, kind: FlashKind, text: impl Into<String>) {
        self.flash = Some(FlashMessage {
            kind,
            text: text.into(),
        });
    }

    pub fn clear_flash(&mut self) {
        self.flash = None;
    }

    pub fn push_parcel(&mut self, entry: ParcelEntry) {
        self.parcels.push(ParcelState {
            entry,
            info: None,
            fetching: false,
            error: None,
            logs: VecDeque::new(),
            show_logs: false,
            min_log_level: LogLevel::Info,
        });
        self.selected = self.parcels.len().saturating_sub(1);
        self.event_scroll = 0;
    }

    pub fn remove_selected(&mut self) -> Option<ParcelEntry> {
        if self.parcels.is_empty() {
            return None;
        }

        let removed = self.parcels.remove(self.selected).entry;
        if self.selected >= self.parcels.len() && !self.parcels.is_empty() {
            self.selected = self.parcels.len() - 1;
        }
        if self.parcels.is_empty() {
            self.selected = 0;
        }
        self.event_scroll = 0;
        Some(removed)
    }

    pub fn push_log_for_index(
        &mut self,
        index: usize,
        level: LogLevel,
        message: impl Into<String>,
    ) {
        let Some(parcel) = self.parcels.get_mut(index) else {
            return;
        };

        if parcel.logs.len() >= MAX_LOG_ENTRIES {
            let _ = parcel.logs.pop_front();
        }
        parcel.logs.push_back(LogEntry {
            timestamp: Utc::now(),
            level,
            message: message.into().replace('\n', " "),
        });
    }

    pub fn push_log_for_selected(&mut self, level: LogLevel, message: impl Into<String>) {
        if self.parcels.is_empty() {
            return;
        }
        self.push_log_for_index(self.selected, level, message);
    }

    pub fn toggle_logs_for_selected(&mut self) -> Option<bool> {
        let selected = self.selected_parcel_mut()?;
        selected.show_logs = !selected.show_logs;
        Some(selected.show_logs)
    }

    pub fn selected_logs_visible(&self) -> bool {
        self.selected_parcel()
            .is_some_and(|parcel| parcel.show_logs)
    }

    pub fn increase_selected_log_level(&mut self) -> Option<LogLevel> {
        let selected = self.selected_parcel_mut()?;
        selected.min_log_level = selected.min_log_level.increase();
        Some(selected.min_log_level)
    }

    pub fn decrease_selected_log_level(&mut self) -> Option<LogLevel> {
        let selected = self.selected_parcel_mut()?;
        selected.min_log_level = selected.min_log_level.decrease();
        Some(selected.min_log_level)
    }

    pub fn selected_log_level(&self) -> Option<LogLevel> {
        self.selected_parcel().map(|parcel| parcel.min_log_level)
    }

    pub fn tick_spinner(&mut self) {
        self.spinner_tick = self.spinner_tick.wrapping_add(1);
    }

    pub fn spinner_char(&self) -> char {
        const FRAMES: [char; 4] = ['-', '\\', '|', '/'];
        FRAMES[self.spinner_tick % FRAMES.len()]
    }
}

impl ParcelState {
    pub fn status_tag(&self) -> &'static str {
        match self.info.as_ref().map(|info| &info.status) {
            Some(TrackingStatus::Delivered) => "[OK]",
            Some(TrackingStatus::InTransit) | Some(TrackingStatus::OutForDelivery) => "[>>]",
            Some(TrackingStatus::Exception) => "[!!]",
            Some(TrackingStatus::PreShipment) => "[..]",
            Some(TrackingStatus::Unknown) => "[~~]",
            None => "[--]",
        }
    }
}
