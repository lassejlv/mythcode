//! Interactive select mode for model picker, session picker, etc.

pub(super) struct SelectMode {
    pub title: String,
    pub items: Vec<SelectItem>,
    pub filtered: Vec<usize>,
    pub filter: String,
    pub selected: usize,
    pub kind: SelectKind,
}

#[derive(Clone)]
pub(super) struct SelectItem {
    pub id: String,
    pub display: String,
}

pub(super) enum SelectKind {
    Model,
    Resume,
}

impl SelectMode {
    pub fn new(title: &str, items: Vec<SelectItem>, kind: SelectKind) -> Self {
        let filtered: Vec<usize> = (0..items.len()).collect();
        Self {
            title: title.to_string(),
            items,
            filtered,
            filter: String::new(),
            selected: 0,
            kind,
        }
    }

    pub fn update_filter(&mut self) {
        let q = self.filter.to_lowercase();
        self.filtered = self
            .items
            .iter()
            .enumerate()
            .filter(|(_, item)| {
                q.is_empty()
                    || item.display.to_lowercase().contains(&q)
                    || item.id.to_lowercase().contains(&q)
            })
            .map(|(i, _)| i)
            .collect();
        self.selected = 0;
    }

    pub fn selected_item(&self) -> Option<&SelectItem> {
        self.filtered
            .get(self.selected)
            .and_then(|&i| self.items.get(i))
    }
}
