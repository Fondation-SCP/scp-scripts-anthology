use clap::Parser;

#[derive(Parser)]
#[command(version = "0.1.0")]
pub struct ForumDlParameters {
    /// Sets the path to the forum, if it differs from the default parameters of Wikidot. Without "/" at the start.
    #[arg(long, default_value = "forum:start")]
    pub forum_path: String,
    /// Also downloads hidden threads.
    #[arg(long, short = 'H', default_value = "false")]
    pub hidden: bool,
}