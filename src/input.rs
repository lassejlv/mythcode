use std::io::IsTerminal;
use std::path::Path;

use anyhow::Result;
use ignore::WalkBuilder;

use crate::types::SlashCommand;

const MAX_MENU_ITEMS: usize = 8;

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

    pub fn search(&self, query: &str) -> Vec<String> {
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

pub fn is_interactive_terminal() -> bool {
    std::io::stdin().is_terminal() && std::io::stdout().is_terminal()
}

pub fn score_command(command: &SlashCommand, query: &str) -> Option<(u8, u8, String)> {
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

pub fn mention_query(input: &str) -> Option<(usize, usize, &str)> {
    let start = input
        .char_indices()
        .rev()
        .find(|(_, ch)| ch.is_whitespace())
        .map_or(0, |(index, ch)| index + ch.len_utf8());

    let token = &input[start..];
    let query = token.strip_prefix('@')?;
    Some((start, input.len(), query))
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
