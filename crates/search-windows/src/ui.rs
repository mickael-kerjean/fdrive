use crate::win32::{self, Dialog, Event, Handler};
use crate::Hit;

pub use crate::win32::error;

pub fn run<'a>(search: impl FnMut(&str) -> Result<Vec<Hit>, String> + 'a) -> Result<(), String> {
    let mut state = State { search: Box::new(search), hits: Vec::new() };
    win32::show_modal(&mut state)
}

type Search<'a> = Box<dyn FnMut(&str) -> Result<Vec<Hit>, String> + 'a>;

struct State<'a> {
    search: Search<'a>,
    hits: Vec<Hit>,
}

impl Handler for State<'_> {
    fn handle(&mut self, dialog: Dialog, event: Event) {
        match event {
            Event::Search => self.on_search(dialog),
            Event::Open => self.on_open(dialog),
            Event::Close => dialog.close(),
        }
    }
}

impl State<'_> {
    fn on_search(&mut self, dialog: Dialog) {
        let term = dialog.query();
        if term.is_empty() {
            return;
        }
        match dialog.busy(&format!("Searching \"{term}\"..."), || (self.search)(&term)) {
            Ok(hits) => {
                dialog.title(&format!("Filestash Search: {} results", hits.len()));
                dialog.show(hits.iter().map(|hit| hit.label.as_str()));
                self.hits = hits;
            }
            Err(err) => {
                dialog.title(&format!("Search failed: {err}"));
                dialog.show([]);
                self.hits.clear();
            }
        }
    }

    fn on_open(&self, dialog: Dialog) {
        if let Some(hit) = dialog.selected().and_then(|index| self.hits.get(index)) {
            dialog.open(&hit.url);
        }
    }
}
