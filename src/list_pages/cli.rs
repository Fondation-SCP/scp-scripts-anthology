use clap::Parser;

#[derive(Parser)]
#[command(version = "0.3.0")]
#[derive(Debug)]
pub struct ListPagesParameters {
    /// Defines the information requested from Crom, separated by spaces or commas.
    #[arg(long, short, default_value = "url wikidotInfo.title", num_args = 1..)]
    pub info: Vec<String>,
    /// Pages must include all following tags.
    #[arg(long, short = 'T', value_name = "TAG", num_args = 1..)]
    pub all_tags: Vec<String>,
    /// Pages must include one of the following tags.
    #[arg(long, short = 't', value_name = "TAG", num_args = 1..)]
    pub one_of_tags: Vec<String>,
    /// Searches within the pages attributed to the given author.
    #[arg(long, short)]
    pub author: Option<String>,
    /// Downloads the contents of each page from the HTML page.
    #[arg(long, default_value = "false")]
    pub content: bool,
    /// Downloads the full HTML of each page and stores it the given folder.
    #[arg(long, default_value = None)]
    pub download_html: Option<String>,
    /// Downloads the sources of fragmented pages. Adds wikidotInfo.source to --info if not specified.
    #[arg(long, default_value = "false")]
    pub gather_fragments_sources: bool,
    /// Removes from the results all pages not containing all given regexes. Adds wikidotInfo.source to --info if not specified.
    #[arg(long, value_name = "REGEX", num_args = 1..)]
    pub source_contains: Vec<String>,
    /// Changes the behavior of --source-contains (removes pages not containing one of the given strings).
    #[arg(long, default_value = "false", requires = "source_contains")]
    pub source_contains_one: bool,
    /// Ignores case for --source-contains.
    #[arg(long, default_value = "false", requires = "source_contains")]
    pub source_contains_ignore_case: bool,
    /// Sets default parameters to scrap the website for analysis with TXM. Overrides --content, --gather-fragment-sources, --format. Disables --source-contains.
    #[arg(long, default_value = "false")]
    pub txm: bool,
    /// [REQUIRES CHROMIUM] Lists the files of listed pages
    #[arg(long, short, default_value = "false")]
    pub files: bool,
}

impl ListPagesParameters {

    /// Applies automatic inferences linking some params to others
    pub fn apply_inferences(&mut self) {
        if self.txm {
            const TXM_PARAMS: [&str; 7] = [
                "url",
                "wikidotInfo.title",
                "wikidotInfo.rating",
                "wikidotInfo.tags",
                "wikidotInfo.children.url",
                "wikidotInfo.createdAt",
                "wikidotInfo.createdBy.name",
            ];
            self.info = TXM_PARAMS.into_iter().map(String::from).collect();
            self.content = true;
        } else {
            let url_str = "url".to_string();
            if self.content && !self.info.contains(&url_str) {
                self.info.push(url_str);
            }

            let source_str = "wikidotInfo.source".to_string();
            if (!self.source_contains.is_empty() || self.gather_fragments_sources)
                && !self.info.contains(&source_str)
            {
                self.info.push(source_str);
            }

            let children_str = "wikidotInfo.children.url".to_string();
            if self.gather_fragments_sources && !self.info.contains(&children_str) {
                self.info.push(children_str);
            }
        }
    }
}