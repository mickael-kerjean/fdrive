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
                let labels: Vec<String> = hits.iter().map(label).collect();
                dialog.show(labels.iter().map(String::as_str));
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

fn label(hit: &Hit) -> String {
    let date = hit.modified.map(win32::local_time).unwrap_or_default();
    format!("{date}\t{}\t{}", hit.size.map(size).unwrap_or_default(), hit.path)
}

fn size(bytes: u64) -> String {
    let units = ["B", "KB", "MB", "GB", "TB"];
    let (mut value, mut unit) = (bytes as f64, 0);
    while value >= 1024.0 && unit < units.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    match unit {
        0 => format!("{bytes} B"),
        _ => format!("{value:.1} {}", units[unit]),
    }
}
