#[derive(Debug)]
pub struct ScriptData {
    pub site: String,
    pub list_all_pages: String,
    pub other_args: Vec<(String, String)>,
    pub verbose: bool
}