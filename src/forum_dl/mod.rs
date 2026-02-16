mod cli;

use crate::cli::{Cli, Script};
use crate::common_tools;
use crate::common_tools::{download_html, FutureIterator};
use futures_util::StreamExt;
use scraper::{ElementRef, Html, Selector};
use std::iter;
use std::sync::Arc;
use chromiumoxide::serde_json::{Deserialize, Serialize};
use lazy_static::lazy_static;
pub use cli::ForumDlParameters;

#[derive(Serialize, Deserialize)]
struct Category {
    name: String,
    url: String,
    threads_nb: Option<i32>,
    posts: Option<i32>,
    threads: Box<[Thread]>,
}

#[derive(Serialize, Deserialize)]
struct Thread {
    title: String,
    url: String,
    description: String,
    date: String,
    posts_nb: Option<i32>,
    author: String,
    messages: Box<[Message]>,
}

#[derive(Serialize, Deserialize)]
struct Message {
    title: String,
    content: String,
    author: String,
    date: String,
    answers: Box<[Message]>,
}

lazy_static!(
    static ref FDL_SEL_GROUP: Selector = Selector::parse("div.forum-group").unwrap();
    static ref FDL_SEL_TR: Selector = Selector::parse("tr").unwrap();
    static ref FDL_SEL_TITLE: Selector = Selector::parse("td.name div.title a").unwrap();
    static ref FDL_SEL_THREADS: Selector = Selector::parse(".threads").unwrap();
    static ref FDL_SEL_POSTS: Selector = Selector::parse(".posts").unwrap();
);

pub async fn forum_dl(data: Cli) {

    let client = Arc::new(reqwest::Client::new());
    let url = data.site.as_ref().unwrap();
    let forum_dl_parameters = match &data.script {
        Script::ForumDl(e) => e,
        _ => panic!(), /* Impossible, treated in main */
    };
    let forum_path = url.clone()
        + forum_dl_parameters.forum_path.as_str()
        + if forum_dl_parameters.hidden { "/hidden/show" } else { "" };

    if data.verbose {
        eprintln!("Warning: --verbose has no effect for this script.");
    }

    println!("Downloading {forum_path}");

    let doc = download_html(&client, forum_path.as_str(), 5)
        .await
        .expect("Too many failed attempts");

    let groups = doc.select(&FDL_SEL_GROUP);
    let categories: Box<[_]> = groups
        .flat_map(|group| {
            group.select(&FDL_SEL_TR).skip(1).map(|tr| Category {
                name: tr
                    .select(&FDL_SEL_TITLE)
                    .next().map(|title| title.inner_html())
                    .unwrap_or_else(|| panic!("Can't find title for a category: {}", tr.inner_html())),
                url: url.clone() +
                    tr.select(&FDL_SEL_TITLE).next()
                    .and_then(|title| title.attr("href"))
                    .unwrap_or_else(|| panic!("Can't find title for a category: {}", tr.inner_html()))
                    .strip_prefix("/")
                    .expect("Category URL is not relative (but it should be).")
                    .rsplit_once('/')
                    .unwrap()
                    .0,
                threads_nb: tr
                    .select(&FDL_SEL_THREADS).next()
                    .and_then(|threads| threads.inner_html().parse().ok()),
                posts: tr
                    .select(&FDL_SEL_POSTS).next()
                    .and_then(|posts| posts.inner_html().parse().ok()),
                threads: Box::default(),
            })
        })
        .collect();

    println!("Categories found: {}", categories.len());

    let categories = categories
        .into_iter()
        .map(|category| _category_dl(client.clone(), category, url.clone(), data.threads))
        .into_future_iter()
        .buffer_unordered(1)
        .collect::<Vec<_>>()
        .await.into_boxed_slice();

    let path = data.output.path().clone();

    common_tools::write_out(data, &categories);

    println!("Results written in file {}", path);
}


fn _get_page_nb(doc: &Html) -> i32 {
    let sel_pager = Selector::parse(".pager span").unwrap();

    doc.select(&sel_pager)
        .next()
        .and_then(|span| {
            span.inner_html()
                .split(" ")
                .last()
                .and_then(|page_str| page_str.parse::<i32>().ok())
        })
        .unwrap_or(1)
}

lazy_static!(
    static ref GT_SEL_TR: Selector = Selector::parse(".table tr").unwrap();
    static ref GT_SEL_TITLE: Selector = Selector::parse(".name .title a").unwrap();
    static ref GT_SEL_DESC: Selector = Selector::parse(".name .description").unwrap();
    static ref GT_SEL_DATE: Selector = Selector::parse(".started .odate").unwrap();
    static ref GT_SEL_POSTS: Selector = Selector::parse(".posts").unwrap();
    static ref GT_SEL_AUTHOR: Selector = Selector::parse(".started .printuser a").unwrap();
);

async fn _get_threads(client: Arc<reqwest::Client>, url: String, site: String) -> Box<[Thread]> {
    let doc = download_html(client.as_ref(), url.as_str(), 5)
        .await
        .expect("Too many failed attempts");

    doc.select(&GT_SEL_TR)
        .skip(1)
        .map(|thread| {
            let title = thread.select(&GT_SEL_TITLE).next();
            Thread {
                title: title.map(|link| link.inner_html())
                    .expect("No title for a forum thread.")
                    .trim()
                    .to_string(),
                url: site.clone()
                    + title
                        .and_then(|link| link.attr("href"))
                        .expect("No url for a forum thread")
                        .strip_prefix("/")
                        .expect("Thread URL is not relative (but it should be).")
                        .rsplit_once('/')
                        .unwrap()
                        .0,
                description: doc
                    .select(&GT_SEL_DESC)
                    .next().map(|desc| desc.inner_html())
                    .unwrap_or_default()
                    .trim()
                    .to_string(),
                date: doc
                    .select(&GT_SEL_DATE)
                    .next().map(|date| date.inner_html())
                    .unwrap_or_default(),
                posts_nb: doc
                    .select(&GT_SEL_POSTS)
                    .next()
                    .and_then(|posts| posts.inner_html().parse().ok()),
                author: doc
                    .select(&GT_SEL_AUTHOR).nth(1).map(|author| author.inner_html())
                    .unwrap_or_default(),
                messages: Box::default(),
            }
        })
        .collect::<Box<[_]>>()
}

lazy_static!(
    static ref PM_SEL_POST: Selector = Selector::parse(".post").unwrap();
    static ref PM_SEL_CONTAINERS: Selector = Selector::parse(".post-container").unwrap();
    static ref PM_SEL_TITLE: Selector = Selector::parse(".long .head .title").unwrap();
    static ref PM_SEL_DATE: Selector = Selector::parse(".long .head .info .odate").unwrap();
    static ref PM_SEL_AUTHOR: Selector = Selector::parse(".long .head .info .printuser a").unwrap();
    static ref PM_SEL_CONTENT: Selector = Selector::parse(".long .content").unwrap();
);

fn _parse_messages_rec(post_container: ElementRef) -> Message {
    let mut skip = 0;

    let message = post_container
        .select(&PM_SEL_POST)
        .next()
        .or({
            skip += 1;
            post_container.select(&PM_SEL_CONTAINERS).nth(1)
        })
        .expect("No post in a post container.");

    Message {
        title: message
            .select(&PM_SEL_TITLE)
            .next().map(|title| title.inner_html())
            .unwrap_or_default()
            .trim()
            .to_string(),
        date: message
            .select(&PM_SEL_DATE)
            .next().map(|title| title.inner_html())
            .unwrap_or("Unknown date".to_string()),
        author: message
            .select(&PM_SEL_AUTHOR).nth(1).map(|title| title.inner_html())
            .unwrap_or("(account deleted)".to_string()),
        content: message
            .select(&PM_SEL_CONTENT)
            .next().map(|title| title.inner_html())
            .unwrap_or_default()
            .trim()
            .to_string(),
        answers: message
            .select(&PM_SEL_CONTAINERS)
            .skip(skip)
            .map(|container| _parse_messages_rec(container))
            .collect(),
    }
}

lazy_static!(
    static ref GM_SEL_THREAD_CONTAINER_POSTS: Selector = Selector::parse("#thread-container-posts").unwrap();
);

async fn _get_messages(client: Arc<reqwest::Client>, mut thread: Thread) -> Thread {
    let doc = download_html(client.as_ref(), thread.url.as_str(), 5)
        .await
        .expect("Too many failed attempts");
    let pages_nb = _get_page_nb(&doc);

    let full_doc = Html::parse_fragment(
        iter::once(doc)
            .chain(
                (1..=pages_nb)
                    .map(|i| format!("{}/p/{i}", thread.url))
                    .map(async |url| {
                        download_html(client.as_ref(), url.as_str(), 5)
                            .await
                            .expect("Too many failed attempts")
                })
                .join_all().await
            )
            .fold(String::new(), |acc, doc| {
                acc + doc
                    .select(&GM_SEL_THREAD_CONTAINER_POSTS)
                    .fold(String::new(), |acc2, thread_container| {
                        acc2 + thread_container.inner_html().as_str() + "\n"
                    })
                    .as_str()
                    + "\n"
            })
            .as_str(),
    );

    let messages: Box<[_]> = full_doc
        .select(&PM_SEL_CONTAINERS)
        .map(|post_container| _parse_messages_rec(post_container))
        .collect();

    thread.messages = messages;

    thread
}

async fn _category_dl(
    client: Arc<reqwest::Client>,
    mut category: Category,
    site: String,
    max_threads: usize,
) -> Category {
    println!("Category: {}", category.name);
    let doc = download_html(client.as_ref(), category.url.as_str(), 5)
        .await
        .expect("Too many failed attempts");
    let pages_nb = _get_page_nb(&doc);

    let threads = (1..pages_nb + 1)
        .map(|i| format!("{}/p/{i}", category.url))
        .map(|page| _get_threads(client.clone(), page, site.clone()))
        .into_future_iter()
        .buffer_unordered(max_threads)
        .collect::<Vec<_>>()
        .await
        .into_iter()
        .flatten()
        .collect::<Box<[_]>>();

    println!("Threads found: {}", threads.len());

    let threads = threads
        .into_iter()
        .map(|thread| _get_messages(client.clone(), thread))
        .into_future_iter()
        .buffer_unordered(max_threads)
        .collect::<Vec<_>>()
        .await.into_boxed_slice();

    category.threads = threads;

    if category.threads_nb.is_some_and(|len| len != category.threads.len() as i32)  {
        eprintln!(
            "[WARNING] Number of threads found doesn't match number of threads announced by Wikidot."
        )
    }

    category
}