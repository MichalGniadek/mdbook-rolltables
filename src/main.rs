use mdbook::{
    book::{Book, Chapter},
    errors::{Error, Result},
    preprocess::{CmdPreprocessor, Preprocessor, PreprocessorContext},
    BookItem,
};
use pulldown_cmark::{Alignment, CowStr, Event, Options, Parser, Tag};
use pulldown_cmark_to_cmark::cmark;
use semver::{Version, VersionReq};
use serde_json;
use std::{io, iter, process};
use toml::value::Map;

fn main() -> Result<(), Error> {
    let preprocessor = RollTables;

    let mut args = pico_args::Arguments::from_env();
    if args.subcommand()? == Some(String::from("supports")) {
        let args = args.finish();
        let renderer = args.first().expect("Required renderer");
        if preprocessor.supports_renderer(renderer.to_str().unwrap()) {
            process::exit(0);
        } else {
            process::exit(1);
        }
    } else {
        let (ctx, book) = CmdPreprocessor::parse_input(io::stdin())?;

        let book_version = Version::parse(&ctx.mdbook_version)?;
        let version_req = VersionReq::parse(mdbook::MDBOOK_VERSION)?;

        if !version_req.matches(&book_version) {
            eprintln!(
                "Warning: The {} plugin was built against version {} of mdbook, \
                 but we're being called from version {}",
                preprocessor.name(),
                mdbook::MDBOOK_VERSION,
                ctx.mdbook_version
            );
        }

        let processed_book = preprocessor.run(&ctx, book)?;
        serde_json::to_writer(io::stdout(), &processed_book)?;

        Ok(())
    }
}

struct RollTables;

impl Preprocessor for RollTables {
    fn name(&self) -> &str {
        "rolltables"
    }

    fn run(&self, ctx: &PreprocessorContext, mut book: Book) -> Result<Book> {
        let cfg = &Map::new();
        let cfg = ctx.config.get_preprocessor(self.name()).unwrap_or(cfg);

        let label_separator = cfg
            .get("label-separator")
            .map(|v| v.as_str())
            .flatten()
            .unwrap_or(" d");

        let separator = cfg
            .get("separator")
            .map(|v| v.as_str())
            .flatten()
            .unwrap_or(".");

        let allow_unusual_dice = cfg
            .get("allow-unusual-dice")
            .map(|v| v.as_bool())
            .flatten()
            .unwrap_or(false);

        book.for_each_mut(|item| {
            if let BookItem::Chapter(chapter) = item {
                self.handle_chapter(chapter, &label_separator, &separator, allow_unusual_dice)
            }
        });
        Ok(book)
    }
}

impl RollTables {
    fn handle_chapter(
        &self,
        chapter: &mut Chapter,
        label_separator: &str,
        separator: &str,
        allow_unusual_dice: bool,
    ) {
        let mut buf = String::with_capacity(chapter.content.len());

        let mut events = Parser::new_ext(&chapter.content, Options::ENABLE_TABLES);

        let mut state = cmark(iter::empty::<Event>(), &mut buf, None).unwrap();

        while let Some(ev) = events.next() {
            match ev {
                Event::Start(Tag::Table(alignment)) => {
                    let mut table = MarkdownTable::new(alignment.clone(), &mut events);

                    if table.content[0][0][..] == [Event::Text(CowStr::from("d"))]
                        && table.content.iter().skip(1).all(|row| row[0].is_empty())
                    {
                        let count = table.content.len() - 1;
                        let (label, iter) = get_dice_iterator(
                            count,
                            label_separator,
                            separator,
                            allow_unusual_dice,
                        );

                        table.content[0][0] = label;

                        for (row, i) in table.content.iter_mut().skip(1).zip(iter) {
                            row[0] = i;
                        }
                    }

                    state = cmark(table.into_iter(), &mut buf, Some(state)).unwrap();
                }
                _ => state = cmark(iter::once(ev), &mut buf, Some(state)).unwrap(),
            }
        }

        chapter.content = buf;
    }
}

#[derive(Debug, Clone)]
struct MarkdownTable<'a> {
    alignment: Vec<Alignment>,
    content: Vec<Vec<Vec<Event<'a>>>>,
}

impl<'a> MarkdownTable<'a> {
    fn new(alignment: Vec<Alignment>, parser: &mut Parser<'a>) -> Self {
        let mut slf = Self {
            alignment,
            content: vec![],
        };

        loop {
            match parser.next().unwrap() {
                Event::Start(Tag::TableHead) | Event::Start(Tag::TableRow) => {
                    slf.content.push(vec![])
                }
                Event::Start(Tag::TableCell) => slf.content.last_mut().unwrap().push(vec![]),
                Event::End(Tag::TableHead)
                | Event::End(Tag::TableRow)
                | Event::End(Tag::TableCell) => {}
                Event::End(Tag::Table(_)) => break,
                ev => slf.content.last_mut().unwrap().last_mut().unwrap().push(ev),
            }
        }

        slf
    }
}

impl<'a> IntoIterator for MarkdownTable<'a> {
    type Item = Event<'a>;

    type IntoIter = MarkdownTableIterator<'a>;

    fn into_iter(self) -> Self::IntoIter {
        MarkdownTableIterator {
            table: self,
            row: None,
            cell: None,
            cell_event: None,
            finished: false,
        }
    }
}

struct MarkdownTableIterator<'a> {
    table: MarkdownTable<'a>,
    row: Option<usize>,
    cell: Option<usize>,
    cell_event: Option<usize>,
    finished: bool,
}

impl<'a> Iterator for MarkdownTableIterator<'a> {
    type Item = Event<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.finished {
            return None;
        }

        if let Some(row) = self.row {
            if row < self.table.content.len() {
                if let Some(cell) = self.cell {
                    if cell < self.table.content[row].len() {
                        if let Some(cell_event) = self.cell_event {
                            if cell_event < self.table.content[row][cell].len() {
                                self.cell_event = Some(cell_event + 1);
                                return Some(self.table.content[row][cell][cell_event].clone());
                            } else {
                                self.cell_event = None;
                                self.cell = Some(cell + 1);
                                return Some(Event::End(Tag::TableCell));
                            }
                        } else {
                            self.cell_event = Some(0);
                            return Some(Event::Start(Tag::TableCell));
                        }
                    } else {
                        self.cell = None;
                        self.row = Some(row + 1);
                        if row == 0 {
                            return Some(Event::End(Tag::TableHead));
                        } else {
                            return Some(Event::End(Tag::TableRow));
                        }
                    }
                } else {
                    self.cell = Some(0);
                    if row == 0 {
                        return Some(Event::Start(Tag::TableHead));
                    } else {
                        return Some(Event::Start(Tag::TableRow));
                    }
                }
            } else {
                self.row = None;
                self.finished = true;
                return Some(Event::End(Tag::Table(self.table.alignment.clone())));
            }
        } else {
            self.row = Some(0);
            return Some(Event::Start(Tag::Table(self.table.alignment.clone())));
        }
    }
}

fn get_dice_iterator<'a>(
    count: usize,
    label_separator: &'a str,
    separator: &'a str,
    allow_unusual_dice: bool,
) -> (
    Vec<Event<'a>>,
    Box<dyn Iterator<Item = Vec<Event<'a>>> + 'a>,
) {
    macro_rules! string_to_event_iter {
        ($e:expr) => {
            Box::new(($e).map(|s| vec![Event::Text(CowStr::from(s))]))
        };
    }

    let combined_dice = |a: usize,
                         b: usize|
     -> (
        Vec<Event<'a>>,
        Box<dyn Iterator<Item = Vec<Event<'a>>> + 'a>,
    ) {
        (
            vec![Event::Text(CowStr::from(format!(
                "d{}{}{}",
                a, label_separator, b
            )))],
            string_to_event_iter!((1..=a)
                .into_iter()
                .flat_map(move |d| std::iter::repeat(d).zip(1..=b))
                .map(move |(a, b)| format!("{}{}{}", a, separator, b))),
        )
    };

    match count {
        16 => combined_dice(4, 4),
        24 => combined_dice(6, 4),
        32 => combined_dice(8, 4),
        36 => combined_dice(6, 6),
        48 => combined_dice(8, 6),
        64 => combined_dice(8, 8),
        _ => {
            if !allow_unusual_dice && !matches!(count, 4 | 6 | 8 | 10 | 12 | 20 | 100) {
                eprintln!("Warning: Roll table created with unusual dice: d{}", count);
            }

            (
                vec![Event::Text(CowStr::from(format!("d{}", count)))],
                string_to_event_iter!((1..=count).into_iter().map(|i| format!("{}", i))),
            )
        }
    }
}
