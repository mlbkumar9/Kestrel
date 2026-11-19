//! Page history for the Back and Forward buttons.

use super::Page;

#[derive(Clone, Debug)]
pub struct NavHistory {
    entries: Vec<Page>,
    index: usize,
}

impl NavHistory {
    pub fn new(start: Page) -> Self {
        Self {
            entries: vec![start],
            index: 0,
        }
    }

    pub fn current(&self) -> Page {
        self.entries[self.index]
    }

    pub fn can_back(&self) -> bool {
        self.index > 0
    }

    pub fn can_forward(&self) -> bool {
        self.index + 1 < self.entries.len()
    }

    /// Records a navigation. Going to the current page changes nothing; any forward
    /// entries are dropped.
    pub fn push(&mut self, page: Page) {
        if page == self.current() {
            return;
        }
        self.entries.truncate(self.index + 1);
        self.entries.push(page);
        self.index += 1;
    }

    pub fn back(&mut self) -> Option<Page> {
        if !self.can_back() {
            return None;
        }
        self.index -= 1;
        Some(self.current())
    }

    pub fn forward(&mut self) -> Option<Page> {
        if !self.can_forward() {
            return None;
        }
        self.index += 1;
        Some(self.current())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn starts_without_history() {
        let mut nav = NavHistory::new(Page::Library);
        assert!(!nav.can_back() && !nav.can_forward());
        assert_eq!(nav.back(), None);
        assert_eq!(nav.forward(), None);
    }

    #[test]
    fn push_then_back_and_forward() {
        let mut nav = NavHistory::new(Page::Library);
        nav.push(Page::Remux);
        nav.push(Page::Queue);
        assert_eq!(nav.current(), Page::Queue);
        assert_eq!(nav.back(), Some(Page::Remux));
        assert_eq!(nav.back(), Some(Page::Library));
        assert!(!nav.can_back() && nav.can_forward());
        assert_eq!(nav.forward(), Some(Page::Remux));
        assert_eq!(nav.forward(), Some(Page::Queue));
        assert!(!nav.can_forward());
    }

    #[test]
    fn new_navigation_truncates_forward_entries() {
        let mut nav = NavHistory::new(Page::Library);
        nav.push(Page::Remux);
        nav.push(Page::Queue);
        nav.back();
        nav.back();
        nav.push(Page::Settings);
        assert!(!nav.can_forward());
        assert_eq!(nav.back(), Some(Page::Library));
        assert_eq!(nav.forward(), Some(Page::Settings));
    }

    #[test]
    fn pushing_the_current_page_is_ignored() {
        let mut nav = NavHistory::new(Page::Library);
        nav.push(Page::Library);
        assert!(!nav.can_back());
        nav.push(Page::Remux);
        nav.push(Page::Remux);
        assert_eq!(nav.back(), Some(Page::Library));
        assert_eq!(nav.back(), None);
    }
}
