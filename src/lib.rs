#![warn(missing_docs)]

//! A mdBook preprocessor that makes writing roll tables
//! for RPG books easier.
//!
//! For example it translates this table:
//!
//! |d|Class|
//! |:---:|:---|
//! ||Warrior|
//! ||Thief|
//! ||Wizard|
//!
//! to this one:
//!
//! |d6|Class|
//! |:---:|:---|
//! |1,2|Warrior|
//! |3,4|Thief|
//! |5,6|Wizard|
//!
//! The preprocessor converts only tables where the first column
//! in the header is "d" and the rest of the first column is empty.
//! It will automatically choose a die (or a combination) depending
//! on the number of rows.
//!
//! Supported options:
//! ```toml
//! [preprocessor.rolltables]
//! # Separator when there are multiple dice e.g. d66
//! separator = "."
//! # Separator when there are multiple dice e.g. d66 but in header
//! head-separator = ""
//! # Warns about d7, d9 etc.
//! warn-unusual-dice = true
//! ```

use anyhow::anyhow;
use mdbook::{
    book::{Book, Chapter},
    errors::Result,
    preprocess::{Preprocessor, PreprocessorContext},
    BookItem,
};
use pulldown_cmark::{Alignment, Event, Options, Parser, Tag};
use pulldown_cmark_to_cmark::cmark;
use std::iter;
use toml::Value;

/// The struct that implements Preprocessor trait.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RollTables;

impl Preprocessor for RollTables {
    fn name(&self) -> &str {
        "rolltables"
    }

    fn run(&self, ctx: &PreprocessorContext, mut book: Book) -> Result<Book> {
        let cfg = ctx.config.get_preprocessor(self.name()).unwrap();

        let head_separator = match cfg.get("head-separator") {
            Some(Value::String(s)) => s.clone(),
            Some(_) => Err(anyhow!("head-separator must be a string"))?,
            None => "".into(),
        };

        let separator = match cfg.get("separator") {
            Some(Value::String(s)) => s.clone(),
            Some(_) => Err(anyhow!("separator must be a string"))?,
            None => ".".into(),
        };

        let warn_unusual_dice = match cfg.get("warn-unusual-dice") {
            Some(Value::Boolean(b)) => *b,
            Some(_) => Err(anyhow!("warn-unusual-dice must be a bool"))?,
            None => false,
        };

        book.for_each_mut(|item| {
            if let BookItem::Chapter(chapter) = item {
                self.handle_chapter(chapter, &head_separator, &separator, warn_unusual_dice)
            }
        });

        Ok(book)
    }
}

impl RollTables {
    fn handle_chapter(
        &self,
        chapter: &mut Chapter,
        head_separator: &str,
        separator: &str,
        warn_unusual_dice: bool,
    ) {
        let mut buf = String::with_capacity(chapter.content.len());

        let mut events = Parser::new_ext(&chapter.content, Options::ENABLE_TABLES);

        let mut state = cmark(iter::empty::<Event>(), &mut buf, None).unwrap();

        while let Some(ev) = events.next() {
            if let Event::Start(Tag::Table(alignment)) = ev {
                let mut table = MarkdownTable::new(alignment, &mut events);

                if table.head()[0] == [Event::Text("d".into())]
                    && table.rows().iter().all(|row| row[0].is_empty())
                {
                    let count = table.rows().len();
                    let (head, iter) =
                        get_dice_iterator(count, head_separator, separator, warn_unusual_dice);

                    table.head_mut()[0] = head;

                    for (i, row) in iter.zip(table.rows_mut()) {
                        row[0] = i;
                    }
                }

                state = cmark(table.events_iter(), &mut buf, Some(state)).unwrap();
            } else {
                state = cmark(iter::once(ev), &mut buf, Some(state)).unwrap();
            }
        }

        chapter.content = buf;
    }
}

#[derive(Debug, Clone, PartialEq)]
struct MarkdownTable<'a> {
    alignment: Vec<Alignment>,
    content: Vec<Vec<Vec<Event<'a>>>>,
}

impl<'a> MarkdownTable<'a> {
    fn new(alignment: Vec<Alignment>, parser: &mut Parser<'a>) -> Self {
        let mut content = vec![];

        loop {
            match parser.next().unwrap() {
                Event::Start(Tag::TableHead | Tag::TableRow) => content.push(vec![]),
                Event::Start(Tag::TableCell) => content.last_mut().unwrap().push(vec![]),
                Event::End(Tag::TableHead | Tag::TableRow | Tag::TableCell) => {}
                Event::End(Tag::Table(_)) => break Self { alignment, content },
                ev => content.last_mut().unwrap().last_mut().unwrap().push(ev),
            }
        }
    }

    fn head(&self) -> &[Vec<Event<'a>>] {
        &self.content[0][..]
    }

    fn head_mut(&mut self) -> &mut [Vec<Event<'a>>] {
        &mut self.content[0][..]
    }

    fn rows(&self) -> &[Vec<Vec<Event<'a>>>] {
        &self.content[1..]
    }

    fn rows_mut(&mut self) -> &mut [Vec<Vec<Event<'a>>>] {
        &mut self.content[1..]
    }

    fn events_iter(&'a self) -> impl Iterator<Item = Event<'a>> {
        fn cell_events_iter<'b>(cell: &'b Vec<Event<'b>>) -> impl Iterator<Item = Event<'b>> {
            iter::empty()
                .chain(iter::once(Event::Start(Tag::TableCell)))
                .chain(cell.iter().cloned())
                .chain(iter::once(Event::End(Tag::TableCell)))
        }

        iter::empty()
            .chain(iter::once(Event::Start(Tag::Table(self.alignment.clone()))))
            // Head
            .chain(iter::once(Event::Start(Tag::TableHead)))
            .chain(self.head().iter().flat_map(cell_events_iter))
            .chain(iter::once(Event::End(Tag::TableHead)))
            // Rows
            .chain(self.rows().iter().flat_map(|row| {
                // Row
                iter::empty()
                    .chain(iter::once(Event::Start(Tag::TableRow)))
                    .chain(row.iter().flat_map(cell_events_iter))
                    .chain(iter::once(Event::End(Tag::TableRow)))
            }))
            .chain(iter::once(Event::End(Tag::Table(self.alignment.clone()))))
    }
}

fn get_dice_iterator<'a>(
    count: usize,
    head_separator: &'a str,
    separator: &'a str,
    warn_unusual_dice: bool,
) -> (
    Vec<Event<'a>>,
    Box<dyn Iterator<Item = Vec<Event<'a>>> + 'a>,
) {
    fn map_string_to_event<'b>(
        iter: impl Iterator<Item = String> + 'b,
    ) -> Box<dyn Iterator<Item = Vec<Event<'b>>> + 'b> {
        Box::new(iter.map(|s| vec![Event::Text(s.into())]))
    }

    let combined_dice = |a: usize, b: usize| {
        (
            vec![Event::Text(format!("d{}{}{}", a, head_separator, b).into())],
            map_string_to_event(
                (1..=a)
                    .flat_map(move |die| iter::repeat(die).zip(1..=b))
                    .map(move |(n0, n1)| format!("{}{}{}", n0, separator, n1)),
            ),
        )
    };

    match count {
        16 => combined_dice(4, 4),
        24 => combined_dice(6, 4),
        32 => combined_dice(8, 4),
        36 => combined_dice(6, 6),
        48 => combined_dice(8, 6),
        64 => combined_dice(8, 8),
        3 => (
            vec![Event::Text("d6".into())],
            map_string_to_event(
                vec![
                    String::from("1, 2"),
                    String::from("3, 4"),
                    String::from("5, 6"),
                ]
                .into_iter(),
            ),
        ),
        _ => {
            if warn_unusual_dice && ![4, 6, 8, 10, 12, 20, 100].contains(&count) {
                eprintln!("Warning: Roll table created with unusual dice: d{}", count);
            }

            (
                vec![Event::Text(format!("d{}", count).into())],
                map_string_to_event((1..=count).map(|i| format!("{}", i))),
            )
        }
    }
}
