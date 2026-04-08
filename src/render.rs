use std::io::{self, Write};
use std::time::Duration;

use similar::{ChangeTag, TextDiff};
use tokio::sync::oneshot;
use tokio::task::JoinHandle;

use crate::types::{AppEvent, DiffPreview, PromptResult};

mod style {
    pub const RESET: &str = "\x1b[0m";
    pub const BOLD: &str = "\x1b[1m";
    pub const DIM: &str = "\x1b[2m";
    pub const CYAN: &str = "\x1b[36m";
    pub const BOLD_CYAN: &str = "\x1b[1;36m";
    pub const DIM_CYAN: &str = "\x1b[2;36m";
    pub const GREEN: &str = "\x1b[32m";
    pub const RED: &str = "\x1b[31m";
    pub const BOLD_YELLOW: &str = "\x1b[1;33m";
    pub const DIM_WHITE: &str = "\x1b[2;37m";
    pub const BOLD_MAGENTA: &str = "\x1b[1;35m";
}

use style::*;

pub struct Renderer {
    debug: bool,
    color: bool,
    assistant_open: bool,
    printed_text: bool,
    last_activity: Option<String>,
    spinner: Option<Spinner>,
    turn_count: u32,
}

struct Spinner {
    stop_tx: oneshot::Sender<()>,
    task: JoinHandle<()>,
}

impl Renderer {
    pub fn new(debug: bool, color: bool) -> Self {
        Self {
            debug,
            color,
            assistant_open: false,
            printed_text: false,
            last_activity: None,
            spinner: None,
            turn_count: 0,
        }
    }

    pub fn print_welcome(&self, project_name: &str, model: Option<&str>) {
        if !self.color {
            println!("mini-code · {project_name}");
            if let Some(model) = model {
                println!("model: {model}");
            }
            println!();
            return;
        }

        println!();
        println!(
            "  {BOLD_CYAN}mini-code{RESET} {DIM}·{RESET} {BOLD}{project_name}{RESET}"
        );
        if let Some(model) = model {
            println!("  {DIM}model: {model}{RESET}");
        }
        println!(
            "  {DIM}type {RESET}/help{DIM} for commands{RESET}"
        );
        println!();
    }

    pub fn start_turn(&mut self) {
        self.stop_spinner();
        self.close_assistant_block();

        if self.turn_count > 0 {
            self.print_separator();
        }
        self.turn_count += 1;

        self.assistant_open = false;
        self.printed_text = false;
        self.last_activity = None;

        let color = self.color;
        let (stop_tx, mut stop_rx) = oneshot::channel();
        let task = tokio::task::spawn_local(async move {
            let frames = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
            let mut tick = 0usize;
            let mut interval = tokio::time::interval(Duration::from_millis(80));
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

            loop {
                tokio::select! {
                    _ = &mut stop_rx => {
                        clear_status_line();
                        break;
                    }
                    _ = interval.tick() => {
                        let frame = frames[tick % frames.len()];
                        if color {
                            print!("\r\x1b[2K  {DIM_CYAN}{frame} thinking …{RESET}");
                        } else {
                            print!("\r\x1b[2K  {frame} thinking ...");
                        }
                        let _ = io::stdout().flush();
                        tick += 1;
                    }
                }
            }
        });

        self.spinner = Some(Spinner { stop_tx, task });
    }

    pub fn render_event(&mut self, event: AppEvent) {
        match event {
            AppEvent::AssistantText(text) => self.render_assistant_text(&text),
            AppEvent::Activity(activity) => self.render_activity(&activity),
            AppEvent::ModeChanged(mode) => self.render_activity(&format!("mode: {mode}")),
            AppEvent::SessionTitle(title) => self.render_activity(&format!("session: {title}")),
            AppEvent::ToolDiff(diff) => self.render_diff(&diff),
            AppEvent::PermissionRequest(_) => {}
            AppEvent::Warning(message) => self.render_warning(&message),
            AppEvent::DebugProtocol(message) => {
                if self.debug {
                    self.render_debug(&message);
                }
            }
            AppEvent::ProcessStderr(line) => {
                if self.debug {
                    self.render_debug(&format!("stderr {line}"));
                }
            }
        }
    }

    pub fn finish_turn(&mut self, result: &PromptResult) {
        self.stop_spinner();
        self.close_assistant_block();

        if !self.printed_text {
            println!();
        }

        if matches!(
            result.stop_reason,
            agent_client_protocol::StopReason::Cancelled
        ) {
            if self.color {
                println!("  {DIM}● cancelled{RESET}");
            } else {
                println!("  cancelled");
            }
        }
    }

    pub fn clear_screen(&mut self) {
        self.stop_spinner();
        print!("\x1b[2J\x1b[H");
        let _ = io::stdout().flush();
    }

    pub fn print_status(&mut self, message: &str) {
        self.stop_spinner();
        self.close_assistant_block();
        if self.color {
            println!("  {DIM}● {message}{RESET}");
        } else {
            println!("  {message}");
        }
    }

    pub fn prepare_for_input(&mut self) {
        self.stop_spinner();
        self.close_assistant_block();
    }

    fn close_assistant_block(&mut self) {
        if self.assistant_open {
            if self.color {
                print!("{RESET}");
            }
            println!();
            self.assistant_open = false;
        }
    }

    fn render_assistant_text(&mut self, text: &str) {
        self.stop_spinner();
        if !self.assistant_open {
            if self.color {
                print!("{BOLD}");
            }
            self.assistant_open = true;
        }
        print!("{text}");
        let _ = io::stdout().flush();
        self.printed_text = true;
    }

    fn render_activity(&mut self, activity: &str) {
        if self.last_activity.as_deref() == Some(activity) {
            return;
        }

        self.stop_spinner();
        self.close_assistant_block();

        if self.color {
            println!("  {DIM}● {activity}{RESET}");
        } else {
            println!("  > {activity}");
        }

        self.last_activity = Some(activity.to_string());
    }

    fn render_diff(&mut self, diff: &DiffPreview) {
        self.stop_spinner();
        self.close_assistant_block();

        let path_display = diff.path.display();
        let old_text = diff.old_text.as_deref().unwrap_or("");
        let text_diff = TextDiff::from_lines(old_text, &diff.new_text);
        let groups = text_diff.grouped_ops(3);

        if self.color {
            println!("  {DIM}╭─{RESET} {BOLD_MAGENTA}{path_display}{RESET}");
        } else {
            println!("  --- {path_display}");
        }

        if groups.is_empty() {
            if self.color {
                println!("  {DIM}│ (no changes){RESET}");
            }
        }

        for (group_idx, group) in groups.iter().enumerate() {
            if let (Some(first), Some(last)) = (group.first(), group.last()) {
                let old_start = first.old_range().start + 1;
                let old_len = last.old_range().end - first.old_range().start;
                let new_start = first.new_range().start + 1;
                let new_len = last.new_range().end - first.new_range().start;

                if self.color {
                    if group_idx > 0 {
                        println!("  {DIM}│{RESET}");
                    }
                    println!(
                        "  {DIM}│{RESET} {CYAN}@@ -{old_start},{old_len} +{new_start},{new_len} @@{RESET}"
                    );
                } else {
                    if group_idx > 0 {
                        println!("  |");
                    }
                    println!(
                        "  | @@ -{old_start},{old_len} +{new_start},{new_len} @@"
                    );
                }
            }

            for op in group {
                for change in text_diff.iter_changes(op) {
                    let line_no = match change.tag() {
                        ChangeTag::Delete => format_line_no(change.old_index()),
                        ChangeTag::Insert | ChangeTag::Equal => {
                            format_line_no(change.new_index())
                        }
                    };

                    if self.color {
                        match change.tag() {
                            ChangeTag::Delete => {
                                print!(
                                    "  {DIM}│{RESET} {DIM_WHITE}{line_no}{RESET} {RED}-{change}{RESET}"
                                );
                            }
                            ChangeTag::Insert => {
                                print!(
                                    "  {DIM}│{RESET} {DIM_WHITE}{line_no}{RESET} {GREEN}+{change}{RESET}"
                                );
                            }
                            ChangeTag::Equal => {
                                print!(
                                    "  {DIM}│ {line_no}  {change}{RESET}"
                                );
                            }
                        }
                    } else {
                        match change.tag() {
                            ChangeTag::Delete => print!("  | {line_no} -{change}"),
                            ChangeTag::Insert => print!("  | {line_no} +{change}"),
                            ChangeTag::Equal => print!("  | {line_no}  {change}"),
                        }
                    }

                    if !change.to_string_lossy().ends_with('\n') {
                        println!();
                    }
                }
            }
        }

        if self.color {
            println!("  {DIM}╰─{RESET}");
        } else {
            println!("  ---");
        }
    }

    fn render_warning(&mut self, message: &str) {
        self.stop_spinner();
        self.close_assistant_block();
        if self.color {
            eprintln!("  {BOLD_YELLOW}! {message}{RESET}");
        } else {
            eprintln!("  warning: {message}");
        }
    }

    fn render_debug(&mut self, message: &str) {
        self.stop_spinner();
        self.close_assistant_block();
        if self.color {
            eprintln!("  {DIM}[debug] {message}{RESET}");
        } else {
            eprintln!("  [debug] {message}");
        }
    }

    fn print_separator(&self) {
        if self.color {
            println!("{DIM}  ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─{RESET}");
        } else {
            println!("  ---------------");
        }
    }

    fn stop_spinner(&mut self) {
        if let Some(spinner) = self.spinner.take() {
            let _ = spinner.stop_tx.send(());
            spinner.task.abort();
            clear_status_line();
        }
    }
}

fn clear_status_line() {
    print!("\r\x1b[2K");
    let _ = io::stdout().flush();
}

fn format_line_no(index: Option<usize>) -> String {
    match index {
        Some(i) => format!("{:>4}", i + 1),
        None => "    ".to_string(),
    }
}
