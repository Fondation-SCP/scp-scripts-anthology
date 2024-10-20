use reqwest::blocking as breqwest;
use reqwest::header::USER_AGENT;

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
                        println!("Rate limited by CROM. Waiting 5 minutes.");
                        std::thread::sleep(std::time::Duration::from_secs(300));
                    },
                    None => panic!("Error in the JSON response from CROM: {}", json_res.to_string()),
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


fn crom_pages(verbose: &bool, site: String, filter: Option<String>, author: Option<String>, requested_data: String, after: Option<&str>) -> Vec<serde_json::Value> {
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

    let pages_data = full_data.get("edges")
        .and_then(|edges| edges.as_array())
        .unwrap_or_else(|| panic!("Error in JSON response from CROM: {}", full_data))
        .iter().map(|edge| edge.get("node").unwrap_or_else(|| panic!("Error in JSON response from CROM: {}", edge)).clone());

    let page_info = full_data.get("pageInfo").unwrap_or_else(|| panic!("Error in JSON response from CROM: {}", full_data));

    let has_next_page = page_info.get("hasNextPage")
        .and_then(|has_next_page| has_next_page.as_bool())
        .unwrap_or_else(|| panic!("Error in JSON response from CROM: {}", page_info));

    if has_next_page {
        let next_page = page_info.get("endCursor")
            .and_then(|end_cursor| end_cursor.as_str())
            .unwrap_or_else(|| panic!("Error in JSON response from CROM: {}", page_info));
        pages_data.chain(crom_pages(verbose, site, filter, author, requested_data, Some(next_page))).collect()
    } else {
        pages_data.collect()
    }
}

fn build_crom_query(site: &String, filter: &Option<String>, author: &Option<String>, requested_data: &String, after: &Option<&str>) -> String {
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

    match &author {
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

pub fn pages(verbose: &bool, site: String, filter: Option<String>, author: Option<String>, requested_data: String) -> Vec<serde_json::Value> {
    crom_pages(verbose, site, filter, author, requested_data, None)
}