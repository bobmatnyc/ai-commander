//! Inspect mode for live tmux session viewing.

use super::app::{App, Message, ViewMode};

impl App {
    /// Toggle inspect mode (live tmux view).
    pub fn toggle_inspect_mode(&mut self) {
        match self.view_mode {
            ViewMode::Normal | ViewMode::Sessions => {
                if self.project.is_some() {
                    self.view_mode = ViewMode::Inspect;
                    self.inspect_scroll = 0;
                    self.refresh_inspect_content();
                    self.messages.push(Message::system("Entering inspect mode (F2 to exit)"));
                } else {
                    self.messages.push(Message::system("Connect to a project first"));
                }
            }
            ViewMode::Inspect => {
                self.view_mode = ViewMode::Normal;
                self.messages.push(Message::system("Exited inspect mode"));
            }
        }
    }

    /// Refresh the inspect content from tmux.
    pub fn refresh_inspect_content(&mut self) {
        if let (Some(project), Some(tmux)) = (&self.project, &self.tmux) {
            if let Some(session) = self.sessions.get(project) {
                // Capture more lines for full view
                if let Ok(output) = tmux.capture_output(session, None, Some(200)) {
                    self.inspect_content = output;
                }
            }
        }
    }

    /// Scroll up in inspect mode.
    pub fn inspect_scroll_up(&mut self) {
        let max_scroll = self.inspect_content.lines().count().saturating_sub(1);
        if self.inspect_scroll < max_scroll {
            self.inspect_scroll += 1;
        }
    }

    /// Scroll down in inspect mode.
    pub fn inspect_scroll_down(&mut self) {
        if self.inspect_scroll > 0 {
            self.inspect_scroll -= 1;
        }
    }

    /// Scroll up by a page in inspect mode.
    pub fn inspect_scroll_page_up(&mut self, page_size: usize) {
        let max_scroll = self.inspect_content.lines().count().saturating_sub(1);
        self.inspect_scroll = self.inspect_scroll.saturating_add(page_size).min(max_scroll);
    }

    /// Scroll down by a page in inspect mode.
    pub fn inspect_scroll_page_down(&mut self, page_size: usize) {
        self.inspect_scroll = self.inspect_scroll.saturating_sub(page_size);
    }
}
