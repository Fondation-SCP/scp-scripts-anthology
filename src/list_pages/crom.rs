use std::error::Error;
use std::fmt::{Display, Formatter};
use std::time::Duration;
use chromiumoxide::spider_fingerprint::http::header::USER_AGENT;
use serde_json::Value;

const CROM_URL: &str = "https://api.crom.avn.sh/graphql";

#[derive(Debug)]
pub struct Crom {
    client: reqwest::Client,
    verbose: bool
}

impl Crom {
    pub fn new(verbose: bool) -> Self {
        Self {
            client: reqwest::Client::new(),
            verbose
        }
    }

    pub async fn query(&self, request: &str) -> Value {
        crate::common_tools::retry_async(5, Some(Duration::from_secs(10)), async || {
            self._wait_for_ratelimit().await;
            let res: Value = self.client
                .post(CROM_URL)
                .header(USER_AGENT, "ScpScriptAnthology/1.0")
                .json(&serde_json::json!({"query": request}))
                .send().await
                .inspect_err(|e| eprintln!("Request error: {e}. Retrying in 10 seconds."))?
                .json().await
                .inspect_err(|e| eprintln!("Recieved data is not in JSON? {e} Retrying in 10 seconds."))?;

            if let Some(errors) = res.get("errors") {
                eprintln!("Crom returned error(s): {errors}. Retrying.");
                Err(Box::<dyn Error>::from(CromError { errors: errors.to_string() }))
            } else {
                Ok(res)
            }
        }).await
            .expect("Too many failed attempts: giving up.")
    }

    pub async fn _wait_for_ratelimit(&self) {
        const RATE_LIMIT_REQUEST: &str = "query {rateLimit{remaining, resetAt}}";

        loop {
            let response = crate::common_tools::retry_async(5, Some(Duration::from_secs(10)), async ||
                self.client
                    .post(CROM_URL)
                    .header(USER_AGENT, "ScpScriptAnthology/1.0")
                    .json(&serde_json::json!({"query": RATE_LIMIT_REQUEST}))
                    .send()
                    .await
                    .inspect_err(|e| eprintln!("Request error: {e}."))
            ).await.expect("Too many failed attempts: giving up.");

            let json_res: Value = response
                .json()
                .await
                .expect("Recieved data is not JSON?");

            let remaining = json_res.get("data").and_then(|data| {
                data.get("rateLimit")
                    .and_then(|ratelimit| ratelimit.get("remaining"))
                    .and_then(Value::as_u64)
            });

            match (remaining, json_res.get("errors")) {
                (Some(0), _) => {
                    println!("Rate limited by Crom. Waiting 5 minutes.");
                    tokio::time::sleep(Duration::from_secs(300)).await;
                }
                (None, Some(errors)) => {
                    eprintln!("Warning: Crom might be flooded! Waiting 15 seconds.\n{errors}");
                    tokio::time::sleep(Duration::from_secs(15)).await;
                    eprintln!("Retrying.");
                }
                (None, None) => panic!(
                    "No ratelimit nor errors founds in CROM response: {}",
                    json_res
                ),
                _ => break, // Not rate limited
            }
        }
    }

    pub fn build_crom_query(
        site: &str,
        filter: Option<&str>,
        author: Option<&str>,
        requested_data: &str,
        after: Option<&str>,
    ) -> String {
        let query_body = format!(
            "edges {{
          node {{ {requested_data} }}
        }},
        pageInfo {{
          endCursor,
          hasNextPage
        }}"
        );

        let wikidot_info_filter = filter.map(|filter| format!("wikidotInfo: {filter},")).unwrap_or_default();
        let after_query = after.map(|after| format!("after: \"{after}\",")).unwrap_or_default();

        match author {
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

    pub async fn _get_fragment_source(&self, fragment: &Value) -> String {
        let query = &format!(
            "
                        query {{
                            page(url:{}){{
                                wikidotInfo {{ source }}
                            }}
                        }}
                   ",
            fragment.get("url").unwrap()
        );
        if self.verbose {
            println!("Query: {query}");
        }
        let mut response = self.query(query).await;
        if self.verbose {
            println!("Response: {response}");
        }
        response
            .get_mut("data")
            .and_then(|d| d.get_mut("page"))
            .and_then(|p| p.get_mut("wikidotInfo"))
            .and_then(|wi| wi.get_mut("source"))
            .map(Value::take)
            .and_then(|source| source.as_str().map(String::from))
            .unwrap_or_else(|| panic!("Error in JSON response from CROM while querying a fragment {}",
                                      fragment.get("url").unwrap()))
    }
}

#[derive(Debug)]
pub enum QueryTree {
    Node(String),
    MotherNode(String, Vec<QueryTree>),
    None,
}

impl Display for QueryTree {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let str = match self {
            QueryTree::Node(node) => format!("{node},"),
            QueryTree::MotherNode(node, children) => format!(
                "{node} {{ {} }},",
                children
                    .iter()
                    .fold(String::new(), |acc, node| acc + node.to_string().as_str())
            ),
            QueryTree::None => String::new(),
        };
        write!(f, "{}", str)
    }
}

impl QueryTree {

    /* TODO: relire cette fonction */
    fn _gciq_rec_fold<'a>(mut acc: Vec<QueryTree>, item: impl AsRef<[&'a str]>) -> Vec<QueryTree> {
        match item.as_ref() {
            [] => {
                acc.push(QueryTree::None);
            }
            [one] => {
                acc.push(QueryTree::Node(one.to_string()));
            }
            [first, rest @ ..] => {
                if acc.iter().any(|node| {
                    if let QueryTree::MotherNode(str, _) = node {
                        str.as_str() == *first
                    } else {
                        false
                    }
                }) {
                    acc = acc
                        .into_iter()
                        .map(|node| {
                            if let QueryTree::MotherNode(str, vec) = node {
                                if str == *first {
                                    QueryTree::MotherNode(str, Self::_gciq_rec_fold(vec, rest))
                                } else {
                                    QueryTree::MotherNode(str, vec)
                                }
                            } else {
                                node
                            }
                        })
                        .collect();
                } else {
                    acc.push(QueryTree::MotherNode(
                        first.to_string(),
                        Self::_gciq_rec_fold(Vec::new(), rest),
                    ));
                }
            }
        }
        acc
    }

    pub fn from_vec(info_list: Vec<&str>) -> Vec<Self> {
        info_list
            .into_iter()
            .map(|info| info.split(".").collect::<Box<[_]>>())
            .fold(Vec::new(), Self::_gciq_rec_fold)
    }


}

#[derive(Debug)]
pub struct CromError {
    errors: String
}

impl Display for CromError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.errors)
    }
}

impl Error for CromError {}