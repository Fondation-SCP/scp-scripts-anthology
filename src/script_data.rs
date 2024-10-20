
#[derive(Debug)]
pub enum OutputFormat {
    JSON,
    YAML,
    /* Custom */ // To add one day with %t etc
}

#[derive(Debug)]
pub struct ScriptData {
    pub site: String,
    pub other_args: Vec<(String, String)>,
    pub verbose: bool,
    pub output_format: OutputFormat,
    pub output_path: Option<String>
}