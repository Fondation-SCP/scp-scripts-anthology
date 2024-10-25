
#[derive(Debug, PartialEq, Eq)]
pub enum OutputFormat {
    JSON,
    YAML,
    Text
    /* Custom */ // To add one day with %t etc
}

#[derive(Debug)]
pub struct ScriptData<'a> {
    pub site: String,
    pub other_args: Vec<(&'a str, &'a str)>,
    pub verbose: bool,
    pub output_format: OutputFormat,
    pub output_path: Option<&'a str>
}