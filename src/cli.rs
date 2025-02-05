use clap::{Parser, ValueEnum};
use clap::Subcommand;
use clio::Output;
use crate::list_pages;

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
        }.to_string()
    }
}

#[derive(Debug, PartialEq, ValueEnum, Clone)]
pub enum OutputFormat {
    JSON,
    YAML
}

#[derive(Subcommand)]
pub enum Script {
    ListPages(list_pages::ListPagesParameters)
}

#[derive(Parser)]
#[command(version)]
pub struct Cli {
    /// The branch you want to use the script on. Overrides --site.
    #[arg(value_enum, short, long, required_unless_present = "site", ignore_case = true)]
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
    #[arg(value_enum, short = 'f', long, default_value = "yaml", ignore_case = true)]
    pub output_format: OutputFormat,
    #[command(subcommand)]
    pub script: Script,
}