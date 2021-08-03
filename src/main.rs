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
use toml::{value::Map, Value};

fn main() -> Result<(), Error> {
    let preprocessor = RollTables;

    let mut args = pico_args::Arguments::from_env();
    if args.subcommand()? == Some(String::from("supports")) {
        let args = args.finish();
        let renderer = args.first().expect("Missing argument");
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
            .and_then(Value::as_str)
            .unwrap_or(" d");

        let separator = cfg.get("separator").and_then(Value::as_str).unwrap_or(".");

        let allow_unusual_dice = cfg
            .get("allow-unusual-dice")
            .and_then(Value::as_bool)
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
            if let Event::Start(Tag::Table(alignment)) = ev {
                let mut table = MarkdownTable::new(alignment, &mut events);

                if table.head()[0] == [Event::Text(CowStr::from("d"))]
                    && table.rows().iter().all(|row| row[0].is_empty())
                {
                    let count = table.rows().len();
                    let (label, iter) =
                        get_dice_iterator(count, label_separator, separator, allow_unusual_dice);

                    table.head_mut()[0] = label;

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

#[derive(Debug, Clone)]
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
        fn cell_events<'b>(cell: &'b Vec<Event<'b>>) -> impl Iterator<Item = Event<'b>> {
            iter::empty()
                .chain(iter::once(Event::Start(Tag::TableCell)))
                .chain(cell.iter().cloned())
                .chain(iter::once(Event::End(Tag::TableCell)))
        }

        iter::empty()
            .chain(iter::once(Event::Start(Tag::Table(self.alignment.clone()))))
            // Head
            .chain(iter::once(Event::Start(Tag::TableHead)))
            .chain(self.head().iter().flat_map(cell_events))
            .chain(iter::once(Event::End(Tag::TableHead)))
            // Rows
            .chain(self.rows().iter().flat_map(|row| {
                // Row
                iter::empty()
                    .chain(iter::once(Event::Start(Tag::TableRow)))
                    .chain(row.iter().flat_map(cell_events))
                    .chain(iter::once(Event::End(Tag::TableRow)))
            }))
            .chain(iter::once(Event::End(Tag::Table(self.alignment.clone()))))
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
    fn map_string_to_event<'b>(
        iter: impl Iterator<Item = String> + 'b,
    ) -> Box<dyn Iterator<Item = Vec<Event<'b>>> + 'b> {
        Box::new(iter.map(|s| vec![Event::Text(s.into())]))
    }

    let combined_dice = |a: usize, b: usize| {
        (
            vec![Event::Text(
                format!("d{}{}{}", a, label_separator, b).into(),
            )],
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
        _ => {
            if !allow_unusual_dice && ![4, 6, 8, 10, 12, 20, 100].contains(&count) {
                eprintln!("Warning: Roll table created with unusual dice: d{}", count);
            }

            (
                vec![Event::Text(format!("d{}", count).into())],
                map_string_to_event((1..=count).map(|i| format!("{}", i))),
            )
        }
    }
}
