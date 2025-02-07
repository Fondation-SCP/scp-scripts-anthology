use reqwest::blocking as breqwest;
use reqwest::header::USER_AGENT;
use scraper::{Html, Selector};
use serde_json::Value;

fn wait_for_ratelimit(client: &breqwest::Client, crom_url: &str) {
    let mut retries = 0;
    let rate_limit_request = "query {rateLimit{remaining, resetAt}}";
    loop {
        let response = client.post(crom_url)
            .header(USER_AGENT, "ScpScriptAnthology/1.0")
            .json(&serde_json::json!({"query": rate_limit_request}))
            .send();
        match response {
            Err(e) => if retries < 5 {
                eprintln!("Request error: {e}. Retrying in 10 seconds.");
                retries += 1;
                std::thread::sleep(std::time::Duration::from_secs(10));
            } else {panic!("Too many failed attempts: giving up.")},
            Ok(response) => {
                let json_res: serde_json::Value = response.json().unwrap_or_else(|e| panic!("Recieved data is not JSON? Error: {e}"));
                let remaining = json_res.get("data")
                    .and_then(|data| data.get("rateLimit")
                        .and_then(|ratelimit| ratelimit.get("remaining")
                            .and_then(|remaining| remaining.as_u64())));
                match remaining {
                    Some(0) => {
                        println!("Rate limited by Crom. Waiting 5 minutes.");
                        std::thread::sleep(std::time::Duration::from_secs(300));
                    },
                    None => match json_res.get("errors") {
                        Some(errors) => {
                            eprintln!("Warning: Crom might be flooded! Waiting 15 seconds.\n{errors}");
                            std::thread::sleep(std::time::Duration::from_secs(15));
                            eprintln!("Retrying.");
                        },
                        None => panic!("Error in the JSON response from CROM: {}", json_res.to_string()),
                    }
                    _ => break // Not rate limited
                }
            }
        }
    }
}

pub fn query_crom(request: &String) -> serde_json::Value {
    let crom_url = "https://api.crom.avn.sh/graphql";
    let mut retries = 0;
    let client = breqwest::Client::new();
    wait_for_ratelimit(&client, &crom_url);

    loop {
        let response = client.post(crom_url)
            .header(USER_AGENT, "ScpScriptAnthology/1.0")
            .json(&serde_json::json!({"query": request}))
            .send();
        match response {
            Err(e) => if retries < 5 {
                eprintln!("Request error: {e}. Retrying in 10 seconds.");
                retries += 1;
                std::thread::sleep(std::time::Duration::from_secs(10));
            } else {panic!("Too many failed attemps: giving up.")},
            Ok(response) => break response.json().unwrap_or_else(|e| panic!("Recieved data is not JSON? Error: {e}"))
        }
    }
}

fn download_content(url: &String) -> Option<String> {
    /* Downloading html */
    let mut retries = 0;
    let client = breqwest::Client::new();
    let html = loop {
        let response = client.get(url)
            .header(USER_AGENT, "ScpScriptAnthology/1.0")
            .send();
        match response {
            Err(e) => if retries < 5 {
                eprintln!("Content downlaod error: {e}. Retrying in 10 seconds.");
                retries += 1;
                std::thread::sleep(std::time::Duration::from_secs(10));
            } else {panic!("Too many failed attemps: giving up.")},
            Ok(response) => match response.text() {
                Err(e) => if retries < 5 {
                    eprintln!("Content format error: {e}. Retrying in 10 seconds.");
                    retries += 1;
                    std::thread::sleep(std::time::Duration::from_secs(10));
                } else {panic!("Too many failed attemps: giving up.")},
                Ok(text) => break text
            }
        }
    };

    /* Extracting content */
    let page_content_sel = Selector::parse("#page-content").unwrap();
    let doc = Html::parse_document(html.as_str());
    let doc = doc.select(&page_content_sel).next();
    if doc.is_none() {
        eprintln!("No #page-content found for {url}.");
        return None;
    }
    let doc = doc.unwrap();

    let deletion_selectors = vec![
        Selector::parse(".creditRate"),
        Selector::parse(".code"),
        Selector::parse(".footer-wikiwalk-nav")
    ];

    Some(
        Html::parse_fragment(
            deletion_selectors.into_iter().fold(doc.html(), |collector, selector| {
            doc.select(&selector.unwrap()).fold(collector, |collector, element| {
                collector.replace(&element.html(), "")
            })
        }).as_str()).root_element().text().collect()
    )

}

fn crom_pages(verbose: &bool, site: &String, filter: Option<String>, author: Option<&String>, requested_data: String, gather_fragments_sources: bool, download_content: bool, after: Option<&str>) -> Vec<Value> {
    let query = build_crom_query(&site, &filter, &author, &requested_data, &after);
    let response = query_crom(&query);
    if *verbose {
        println!("Query: {query}");
        println!("Response: {response}");
    }
    let full_data = response.get("data")
        .and_then(|data|
            if author.is_some() {
                data.get("user")
                    .and_then(|user| user.get("attributedPages"))
            } else {
                data.get("pages")
            }
        ).unwrap_or_else(|| panic!("Error in JSON response from CROM: {}\nQuery: {query}", response));

    let mut pages_data: Vec<Value> = full_data.get("edges")
        .and_then(|edges| edges.as_array())
        .unwrap_or_else(|| panic!("Error in JSON response from CROM: {}", full_data))
        .iter().map(|edge| edge.get("node").unwrap_or_else(|| panic!("Error in JSON response from CROM: {}", edge)).clone()).collect();

    if download_content && !gather_fragments_sources /* if true, content will be downloaded in the next if block */ {
        pages_data.iter_mut().for_each(|page| {
            let content = self::download_content(
                &page.get("url")
                    .and_then(|j| j.as_str())
                    .unwrap_or_else(|| panic!("Error in JSON response from CROM (--content needs --info with url): {}", page))
                    .to_string()
            ).unwrap_or_else(|| {
                eprintln!("Warning: error when retrieving content for page: {}", page);
                String::from("Error")
            });
            assert!(page.is_object(), "Error in JSON response from CROM (not an object): {}", page);
            page.as_object_mut().unwrap().insert("content".to_string(), Value::String(content));
        });
    }

    if gather_fragments_sources {
        pages_data.iter_mut().for_each(|page| {
            assert!(page.is_object(), "Error in JSON response from CROM (not an object): {}", page);
            let children: Vec<_> = page.get("wikidotInfo")
                .and_then(|wikidotinfo| wikidotinfo.get("children"))
                .and_then(|children| children.as_array())
                .and_then(|children| Some(
                    children.into_iter().filter(|child|
                    child.get("url")
                        .and_then(|url| url.as_str())
                        .is_some_and(|url| url.contains("fragment:"))
                    ).collect()
                )).unwrap_or(Vec::new());

            let mut newsource = None;

            if page.get("wikidotInfo").and_then(|wikidot_info| wikidot_info.get("source")).is_some() {
                newsource = children.iter().map(|fragment| {
                    let query = &format!("
                            query {{
                                page(url:{}){{
                                    wikidotInfo {{ source }}
                                }}
                            }}
                       ", fragment.get("url").unwrap());
                    if *verbose {
                        println!("Query: {query}");
                    }
                    let response = query_crom(query);
                    if *verbose {
                        println!("Response: {response}");
                    }
                    response.get("data")
                        .and_then(|d| d.get("page"))
                        .and_then(|p| p.get("wikidotInfo"))
                        .and_then(|wi| wi.get("source"))
                        .and_then(|source| source.as_str())
                        .map(|source| source.to_string())
                        .unwrap_or_else(|| panic!("Error in JSON response from CROM while querying a fragment {}", fragment.get("url").unwrap()))
                }).reduce(|collector, part| collector + "\n" + part.as_str());

            }

            if download_content {
                let newcontent = children.iter().map(|fragment|
                    self::download_content(&fragment.get("url").unwrap().as_str().unwrap().to_string())
                ).filter_map(|x| x)
                    .reduce(|collector, part| collector + part.as_str());

                if let Some(newcontent) = newcontent {
                    page.as_object_mut().unwrap().insert("content".to_string(), Value::String(newcontent));
                }
            }

            /* Done last to avoid creating a mutable reference to the page before
               now possible because the "children" reference won't be used anymore */
            if let Some(newsource) = newsource {
                if let Some(oldsource) = page.get_mut("wikidotInfo")
                    .and_then(|wikidot_info| wikidot_info.as_object_mut())
                    .and_then(|wikidot_info| wikidot_info.get_mut("source")) {
                    *oldsource = Value::String(newsource.to_string());
                }
            }
        });
    }

    let page_info = full_data.get("pageInfo").unwrap_or_else(|| panic!("Error in JSON response from CROM: {}", full_data));

    let has_next_page = page_info.get("hasNextPage")
        .and_then(|has_next_page| has_next_page.as_bool())
        .unwrap_or_else(|| panic!("Error in JSON response from CROM: {}", page_info));

    if has_next_page {
        let next_page = page_info.get("endCursor")
            .and_then(|end_cursor| end_cursor.as_str())
            .unwrap_or_else(|| panic!("Error in JSON response from CROM: {}", page_info));
        pages_data.into_iter().chain(crom_pages(verbose, site, filter, author, requested_data, gather_fragments_sources, download_content, Some(next_page))).collect()
    } else {
        pages_data
    }
}

fn build_crom_query(site: &String, filter: &Option<String>, author: &Option<&String>, requested_data: &String, after: &Option<&str>) -> String {
    let query_body = format!(
        "edges {{
          node {{ {requested_data} }}
        }},
        pageInfo {{
          endCursor,
          hasNextPage
        }}"
    );
    let wikidot_info_filter = match filter {
        Some(filter) => format!("wikidotInfo: {filter},"),
        None => "".to_string()
    };
    let after_query = match after {
        Some(after) => format!("after: \"{after}\","),
        None => "".to_string()
    };

    match *author {
        None => format!(
            "query {{
                pages( {after_query} filter: {{ {wikidot_info_filter} url:{{startsWith:\"{site}\"}}}}) {{
                    {query_body}
                }}
            }}"
        ),
        Some(author) => format!(
            "query {{\
                user(name: \"{author}\") {{
                    attributedPages( {after_query} filter: {{ {wikidot_info_filter} url: {{startsWith: \"{site}\"}} }}) {{
                        {query_body}
                    }}
                }}
            }}"
        )
    }
}

pub fn pages(verbose: &bool, site: &String, filter: Option<String>, author: Option<&String>, requested_data: String, gather_fragments_sources: bool, download_content: bool) -> Vec<serde_json::Value> {
    crom_pages(verbose, site, filter, author, requested_data, gather_fragments_sources, download_content, None)
}

pub fn xml_escape(s: &str) -> String {
    s.replace("&", "&amp;").replace("<", "&lt;").replace(">","&gt;").replace('"', "&quot;").replace("'","&apos;")
}

