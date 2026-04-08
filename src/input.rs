use std::io::{self, IsTerminal};
use std::path::Path;

use anyhow::Result;
use ignore::WalkBuilder;
use inquire::autocompletion::{Autocomplete, Replacement};
use inquire::ui::{Color, RenderConfig, StyleSheet, Styled};
use inquire::{InquireError, Select, Text};

use crate::types::{
    ModelOption, PermissionDecision, PermissionRequestView, SessionModels, SlashCommand,
};

const MAX_MENU_ITEMS: usize = 8;

#[derive(Debug)]
pub enum ReadLineOutcome {
    Input(String),
    Interrupt,
    EndOfFile,
}

#[derive(Debug, Clone, Default)]
pub struct FileIndex {
    files: Vec<String>,
}

impl FileIndex {
    pub fn build(cwd: &Path) -> Result<Self> {
        let mut files = Vec::new();
        let walker = WalkBuilder::new(cwd).standard_filters(true).build();

        for entry in walker {
            let Ok(entry) = entry else {
                continue;
            };
            let Some(file_type) = entry.file_type() else {
                continue;
            };
            if !file_type.is_file() {
                continue;
            }

            let Ok(relative) = entry.path().strip_prefix(cwd) else {
                continue;
            };
            let rendered = relative.to_string_lossy().replace('\\', "/");
            if !rendered.is_empty() {
                files.push(rendered);
            }
        }

        files.sort();
        Ok(Self { files })
    }

    fn search(&self, query: &str) -> Vec<String> {
        let query = query.trim().to_lowercase();
        let mut matches = self
            .files
            .iter()
            .filter_map(|path| score_path(path, &query).map(|score| (score, path.clone())))
            .collect::<Vec<_>>();

        matches.sort_by(|(left_score, left_path), (right_score, right_path)| {
            left_score
                .cmp(right_score)
                .then_with(|| left_path.len().cmp(&right_path.len()))
                .then_with(|| left_path.cmp(right_path))
        });

        matches
            .into_iter()
            .take(MAX_MENU_ITEMS)
            .map(|(_, path)| path)
            .collect()
    }
}

pub fn init_theme() {
    let config = RenderConfig::default_colored()
        .with_prompt_prefix(Styled::new("❯").with_fg(Color::LightCyan))
        .with_answered_prompt_prefix(Styled::new("✓").with_fg(Color::DarkGreen))
        .with_highlighted_option_prefix(Styled::new("▸").with_fg(Color::LightCyan))
        .with_help_message(StyleSheet::new().with_fg(Color::DarkGrey))
        .with_answer(StyleSheet::new().with_fg(Color::LightCyan));

    inquire::set_global_render_config(config);
}

pub fn is_interactive_terminal() -> bool {
    io::stdin().is_terminal() && io::stdout().is_terminal()
}

pub fn read_line(
    prompt: &str,
    commands: &[SlashCommand],
    files: &FileIndex,
) -> Result<ReadLineOutcome> {
    let autocomplete = PromptAutocomplete::new(commands.to_vec(), files.clone());

    let result = Text::new(prompt)
        .with_help_message("enter to send · tab to complete")
        .with_autocomplete(autocomplete)
        .prompt();

    match result {
        Ok(line) => Ok(ReadLineOutcome::Input(line)),
        Err(InquireError::OperationInterrupted) => Ok(ReadLineOutcome::Interrupt),
        Err(InquireError::OperationCanceled) => Ok(ReadLineOutcome::EndOfFile),
        Err(error) => Err(error.into()),
    }
}

pub fn pick_model(models: &SessionModels) -> Result<Option<ModelOption>> {
    if models.available.is_empty() {
        return Ok(None);
    }

    let result = Select::new("model", models.available.clone())
        .with_page_size(MAX_MENU_ITEMS)
        .with_help_message("type to filter · enter selects · esc cancels")
        .prompt_skippable();

    map_optional_prompt(result)
}

pub fn pick_permission(request: &PermissionRequestView, color: bool) -> Result<PermissionDecision> {
    let mut options = request.options.clone();
    options.sort_by_key(|option| !option.kind.is_accept());

    if color {
        print!("  \x1b[1;33m⚠ {}\x1b[0m", request.title);
    } else {
        print!("  ! {}", request.title);
    }

    if let Some(subtitle) = &request.subtitle {
        if color {
            print!(" \x1b[2m{subtitle}\x1b[0m");
        } else {
            print!(" ({subtitle})");
        }
    }
    println!();

    if !request.locations.is_empty() {
        if color {
            println!("  \x1b[2m  {}\x1b[0m", request.locations.join(", "));
        } else {
            println!("    {}", request.locations.join(", "));
        }
    }

    let result = Select::new("allow?", options)
        .with_page_size(MAX_MENU_ITEMS)
        .with_help_message("enter selects · esc cancels")
        .prompt_skippable();

    match result {
        Ok(Some(selection)) => Ok(PermissionDecision::Selected(selection.option_id)),
        Ok(None) | Err(InquireError::OperationCanceled) => Ok(PermissionDecision::Cancelled),
        Err(InquireError::OperationInterrupted) => Ok(PermissionDecision::Cancelled),
        Err(error) => Err(error.into()),
    }
}

fn map_optional_prompt<T>(result: Result<Option<T>, InquireError>) -> Result<Option<T>> {
    match result {
        Ok(value) => Ok(value),
        Err(InquireError::OperationCanceled) | Err(InquireError::OperationInterrupted) => Ok(None),
        Err(error) => Err(error.into()),
    }
}

#[derive(Clone)]
struct PromptAutocomplete {
    commands: Vec<SlashCommand>,
    files: FileIndex,
}

impl PromptAutocomplete {
    fn new(commands: Vec<SlashCommand>, files: FileIndex) -> Self {
        Self { commands, files }
    }
}

impl Autocomplete for PromptAutocomplete {
    fn get_suggestions(&mut self, input: &str) -> Result<Vec<String>, inquire::CustomUserError> {
        if input.starts_with('/') && !input.contains(char::is_whitespace) {
            let query = input.trim_start_matches('/').to_lowercase();
            let mut matches = self
                .commands
                .iter()
                .filter_map(|command| score_command(command, &query).map(|score| (score, command)))
                .collect::<Vec<_>>();

            matches.sort_by(|(left_score, left_command), (right_score, right_command)| {
                left_score
                    .cmp(right_score)
                    .then_with(|| left_command.name.cmp(&right_command.name))
            });

            return Ok(matches
                .into_iter()
                .take(MAX_MENU_ITEMS)
                .map(|(_, command)| {
                    if command.hint.is_some() {
                        format!("/{} ", command.name)
                    } else {
                        format!("/{}", command.name)
                    }
                })
                .collect());
        }

        if let Some((start, end, query)) = mention_query(input) {
            return Ok(self
                .files
                .search(query)
                .into_iter()
                .map(|path| {
                    let mut next = String::with_capacity(input.len() + path.len() + 2);
                    next.push_str(&input[..start]);
                    next.push('@');
                    next.push_str(&path);
                    next.push(' ');
                    next.push_str(&input[end..]);
                    next
                })
                .collect());
        }

        Ok(Vec::new())
    }

    fn get_completion(
        &mut self,
        input: &str,
        highlighted_suggestion: Option<String>,
    ) -> Result<Replacement, inquire::CustomUserError> {
        Ok(highlighted_suggestion.or_else(|| Some(input.to_string())))
    }
}

fn mention_query(input: &str) -> Option<(usize, usize, &str)> {
    let start = input
        .char_indices()
        .rev()
        .find(|(_, ch)| ch.is_whitespace())
        .map_or(0, |(index, ch)| index + ch.len_utf8());

    let token = &input[start..];
    let query = token.strip_prefix('@')?;
    Some((start, input.len(), query))
}

fn score_command(command: &SlashCommand, query: &str) -> Option<(u8, u8, String)> {
    if query.is_empty() {
        return Some((
            source_rank(command),
            0,
            command.display_name().to_lowercase(),
        ));
    }

    let name = command.name.to_lowercase();
    let display = command.display_name().to_lowercase();

    let match_rank = if name == query || display == format!("/{query}") {
        0
    } else if name.starts_with(query) {
        1
    } else if display.starts_with(&format!("/{query}")) {
        2
    } else if name.contains(query) {
        3
    } else {
        return None;
    };

    Some((source_rank(command), match_rank, display))
}

fn source_rank(command: &SlashCommand) -> u8 {
    match command.source {
        crate::types::SlashCommandSource::Local => 0,
        crate::types::SlashCommandSource::Agent => 1,
    }
}

fn score_path(path: &str, query: &str) -> Option<(u8, String)> {
    if query.is_empty() {
        return Some((0, path.to_lowercase()));
    }

    let lower = path.to_lowercase();
    let file_name = lower.rsplit('/').next().unwrap_or(&lower);

    let rank = if file_name.starts_with(query) {
        0
    } else if lower.starts_with(query) {
        1
    } else if file_name.contains(query) {
        2
    } else if lower.contains(query) {
        3
    } else {
        return None;
    };

    Some((rank, lower))
}
