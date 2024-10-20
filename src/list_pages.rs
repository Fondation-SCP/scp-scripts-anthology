use crate::script_data::ScriptData;
use crate::common_tools::pages;

pub fn list_pages(script_data: ScriptData) {
    let mut all_tags = Vec::new();
    let mut one_of_tags = Vec::new();
    let mut author = None;

    script_data.other_args.iter().for_each(|(arg, value)| match arg.as_str() {
        "--all-tags" | "--all_tags" | "-T" => all_tags = value.split(" ").collect(),
        "--one-of-tags" | "--one_of_tags" | "-t" => one_of_tags = value.split(" ").collect(),
        "--author" | "-a" => author = Some(value.clone()),
        arg  => eprintln!("Warning: unknown parameter {arg}. Parameter ignored.")
    });

    let filter_and = all_tags.into_iter().fold("".to_string(), |acc, tag| {
        let tag_filter = format!("{{ tags: {{ eq: \"{tag}\" }} }}");
        if acc.is_empty() {
            tag_filter
        } else {
            format!("{{ _and: [{tag_filter}, {acc}] }}")
        }
    });

    let filter_or = one_of_tags.into_iter().fold("".to_string(), |acc, tag| {
        let tag_filter = format!("{{ tags: {{ eq: \"{tag}\" }} }}");
        if acc.is_empty() {
            tag_filter
        } else {
            format!("{{ _or: [{tag_filter}, {acc}] }}")
        }
    });

    let filter = match (filter_or.as_str(), filter_and.as_str()) {
        ("", "") => None,
        ("", yes) | (yes, "") => Some(yes.to_string()),
        (or, and) => Some(format!("{{ _and: [ {and}, {or} ] }}"))
    };

    let info = "url, wikidotInfo {title}".to_string();

    println!("Querying crom for the requested pages…");

    let result = pages(&script_data.verbose, script_data.site, filter, author, info);
    let res_str = if result.is_empty() {
        "No results.".to_string()
    } else {
        result.iter().fold("".to_string(), |str, res| {
            let url = res.get("url")
                .and_then(|u| u.as_str())
                .unwrap_or("[URL not found]");
            let title = res.get("wikidotInfo")
                .and_then(|wikidot_info| wikidot_info.get("title")
                    .and_then(|title_info| title_info.as_str()))
                .unwrap_or("[No title]");
            format!("{str}\n{title} -- {url}")
        })
    };
    println!("Seach results: {res_str}");

}