use mdbook::{
    errors::{Error, Result},
    preprocess::{CmdPreprocessor, Preprocessor},
};
use mdbook_rolltables::RollTables;
use semver::{Version, VersionReq};
use serde_json;
use std::{io, process};

fn main() -> Result<(), Error> {
    let mut args = pico_args::Arguments::from_env();
    if args.contains("-h") || args.contains("--help") {
        eprintln!("mdbook-rolltables is a preprocessor for mdBook and can't be used as a standalone executable");
        process::exit(1);
    } else if args.subcommand()? == Some(String::from("supports")) {
        let renderer: String = args.free_from_str().expect("Missing argument");
        if RollTables.supports_renderer(&renderer) {
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
                "Warning: The {} preprocessor was built against version {} of mdbook, \
                 but the preprocessor is being called from version {}",
                RollTables.name(),
                mdbook::MDBOOK_VERSION,
                ctx.mdbook_version
            );
        }

        let processed_book = RollTables.run(&ctx, book)?;
        serde_json::to_writer(io::stdout(), &processed_book)?;

        Ok(())
    }
}
