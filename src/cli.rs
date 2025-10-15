use crate::{forum_dl, list_files, list_pages};
use clap::Subcommand;
use clap::{Parser, ValueEnum};
use clio::Output;

#[derive(Debug, PartialEq, ValueEnum, Clone)]
pub enum Branch {
    FR,
    EN,
    INT,
}

impl Branch {
    pub fn get_url(&self) -> String {
        match self {
            Self::FR => "http://fondationscp.wikidot.com/",
            Self::EN => "http://scp-wiki.wikidot.com/",
            Self::INT => "http://scp-int.wikidot.com/",
        }
        .to_string()
    }
}

#[derive(Debug, PartialEq, ValueEnum, Clone)]
pub enum OutputFormat {
    JSON,
    YAML,
}

#[derive(Subcommand)]
pub enum Script {
    /// Selects pages with selected criteria and downloads multiple information about them.
    ListPages(list_pages::ListPagesParameters),
    /// Downloads the forum of a Wikidot wiki.
    ForumDl(forum_dl::ForumDlParameters),
    /// Lists the files of selected pages via a ListPages modules located on any Wikidot wiki (does not use Crom).
    ///
    /// You need to have set up a page on the wiki that uses the ListPages module to list all pages
    /// whose files you want to have listed. Put the ListPages module in a div with the class
    /// ssa-list-pages so the script can detect it.
    ListFiles(list_files::ListFilesParameters)
}

#[derive(Parser)]
#[command(version)]
pub struct Cli {
    /// The branch you want to use the script on. Overrides --site.
    #[arg(
        value_enum,
        short,
        long,
        required_unless_present = "site",
        ignore_case = true
    )]
    pub branch: Option<Branch>,
    /// The wikidot website you want to use the script on. Don't forget "/" at the end.
    #[arg(short, long, required_unless_present = "branch")]
    pub site: Option<String>,
    /// Prints in the console CROM queries and their responses.
    #[arg(short, long, default_value = "false")]
    pub verbose: bool,
    /// Writes the output in a given file. Writes on the console output by default.
    #[arg(short, long, default_value = "-")]
    pub output: Output,
    /// The format of the output.
    #[arg(
        value_enum,
        short = 'f',
        long,
        default_value = "yaml",
        ignore_case = true
    )]
    pub output_format: OutputFormat,
    /// Number of parallel threads.
    #[arg(short = 'm', long, default_value = "4")]
    pub threads: usize,
    #[command(subcommand)]
    pub script: Script,
}
