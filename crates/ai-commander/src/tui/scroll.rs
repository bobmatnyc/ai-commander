//! Scrolling operations for TUI output areas.

use super::App;

impl App {
    /// Scroll to the bottom of the output.
    pub fn scroll_to_bottom(&mut self) {
        self.scroll_offset = 0;
    }

    /// Scroll up by one line.
    pub fn scroll_up(&mut self) {
        if self.scroll_offset < self.messages.len().saturating_sub(1) {
            self.scroll_offset += 1;
        }
    }

    /// Scroll down by one line.
    pub fn scroll_down(&mut self) {
        if self.scroll_offset > 0 {
            self.scroll_offset -= 1;
        }
    }

    /// Scroll up by a page.
    pub fn scroll_page_up(&mut self, page_size: usize) {
        self.scroll_offset = self.scroll_offset.saturating_add(page_size)
            .min(self.messages.len().saturating_sub(1));
    }

    /// Scroll down by a page.
    pub fn scroll_page_down(&mut self, page_size: usize) {
        self.scroll_offset = self.scroll_offset.saturating_sub(page_size);
    }
}
