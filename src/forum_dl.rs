use crate::cli::{Cli, OutputFormat, Script};
use crate::common_tools::download_html;
use clap::Parser;
use futures_util::future::join_all;
use futures_util::StreamExt;
use scraper::{ElementRef, Html, Selector};
use serde::Deserialize;
use serde::Serialize;
use std::iter;
use std::sync::Arc;
use std::vec::IntoIter;

#[derive(Parser)]
#[command(version = "0.1.0")]
pub struct ForumDlParameters {
    /// Sets the path to the forum, if it differs from the default parameters of Wikidot. Without "/" at the start.
    #[arg(long, default_value = "forum:start")]
    forum_path: String,
    /// Also downloads hidden threads.
    #[arg(long, short = 'H', default_value = "false")]
    hidden: bool,
}

#[derive(Serialize, Deserialize)]
struct Category {
    name: String,
    url: String,
    threads_nb: i32,
    posts: i32,
    threads: Vec<Thread>
}

#[derive(Serialize, Deserialize)]
struct Thread {
    title: String,
    url: String,
    description: String,
    date: String,
    posts_nb: i32,
    author: String,
    messages: Vec<Message>
}

#[derive(Serialize, Deserialize)]
struct Message {
    title: String,
    content: String,
    author: String,
    date: String,
    answers: Vec<Message>
}

fn _get_page_nb(doc: &Html) -> i32 {
    let sel_pager = Selector::parse(".pager span").unwrap();

    doc.select(&sel_pager).next()
        .and_then(|span| span.inner_html().split(" ").last()
            .and_then(|page_str| page_str.parse::<i32>().ok())
        ).unwrap_or(1)
}

async fn _get_threads(client: Arc<reqwest::Client>, url: String, site: String) -> IntoIter<Thread> {
    let doc = download_html(client.as_ref(), url.as_str(), 5).await
        .unwrap_or_else(|e| panic!("Too many failed attempts: {e}"));
    let sel_tr = Selector::parse(".table tr").unwrap();
    doc.select(&sel_tr).skip(1).map(|thread| {
        let sel_title = Selector::parse(".name .title a").unwrap();
        let title = thread.select(&sel_title).next();
        let sel_desc = Selector::parse(".name .description").unwrap();
        let sel_date = Selector::parse(".started .odate").unwrap();
        let sel_posts = Selector::parse(".posts").unwrap();
        let sel_author = Selector::parse(".started .printuser a").unwrap();
        Thread {
            title: title.and_then(|link| Some(link.inner_html())).expect("No title for a forum thread.").trim().to_string(),
            url: site.clone() + title.and_then(|link| link.attr("href")).expect("No url for a forum thread")
                .strip_prefix("/").expect("Thread URL is not relative (but it should be).")
                .rsplit_once('/').unwrap().0,
            description: doc.select(&sel_desc).next().and_then(|desc| Some(desc.inner_html())).unwrap_or_default().trim().to_string(),
            date: doc.select(&sel_date).next().and_then(|date| Some(date.inner_html())).unwrap_or_default(),
            posts_nb: doc.select(&sel_posts).next().and_then(|posts| posts.inner_html().parse().ok()).unwrap_or(-1),
            author: doc.select(&sel_author).skip(1).next().and_then(|author| Some(author.inner_html())).unwrap_or_default(),
            messages: Vec::new()
        }
    }).collect::<Vec<_>>().into_iter()
}

fn _parse_messages_rec(post_container: ElementRef) -> Message {
    let sel_post = Selector::parse(".post").unwrap();
    let sel_containers = Selector::parse(".post-container").unwrap();

    let mut skip = 0;

    let message = post_container.select(&sel_post).next().or(
        {
            skip += 1;
            post_container.select(&sel_containers).skip(0).next()
        }
    ).expect("No post in a post container.");

    let sel_title = Selector::parse(".long .head .title").unwrap();
    let sel_date = Selector::parse(".long .head .info .odate").unwrap();
    let sel_author = Selector::parse(".long .head .info .printuser a").unwrap();
    let sel_content = Selector::parse(".long .content").unwrap();

    Message {
        title: message.select(&sel_title).next()
                .and_then(|title| Some(title.inner_html()))
                .unwrap_or(String::new()).trim().to_string(),
        date: message.select(&sel_date).next()
                .and_then(|title| Some(title.inner_html()))
                .unwrap_or("Unknown date".to_string()),
        author: message.select(&sel_author).skip(1).next()
                .and_then(|title| Some(title.inner_html()))
                .unwrap_or("(account deleted)".to_string()),
        content: message.select(&sel_content).next()
            .and_then(|title| Some(title.inner_html()))
            .unwrap_or(String::new()).trim().to_string(),
        answers: message.select(&sel_containers).skip(skip).map(|container| _parse_messages_rec(container)).collect()
    }
}

async fn _get_messages(client: Arc<reqwest::Client>, mut thread: Thread) -> Thread {
    let doc = download_html(client.as_ref(), thread.url.as_str(), 5).await
        .unwrap_or_else(|e| panic!("Too many failed attempts: {e}"));
    let pages_nb = _get_page_nb(&doc);

    let sel_thread_container_posts = Selector::parse("#thread-container-posts").unwrap();

    let full_doc = Html::parse_fragment(iter::once(doc).chain(
        join_all((1..pages_nb+1).map(|i| format!("{}/p/{i}", thread.url))
            .map(async |url| download_html(client.as_ref(), url.as_str(), 5).await
                .unwrap_or_else(|e| panic!("Too many failed attempts: {e}")))
        ).await
    ).fold(String::new(), |acc, doc|
        acc + doc.select(&sel_thread_container_posts)
            .fold(String::new(), |acc2, thread_container| acc2 + thread_container.inner_html().as_str() + "\n").as_str() + "\n"
    ).as_str());


    let sel_post_container = Selector::parse(".post-container").unwrap();
    let messages: Vec<_> = full_doc.select(&sel_post_container).map(|post_container|
        _parse_messages_rec(post_container)
    ).collect();

    thread.messages = messages;

    thread
}

async fn _category_dl(client: Arc<reqwest::Client>, mut category: Category, site: String, max_threads: usize) -> Category {
    println!("Category: {}", category.name);
    let doc = download_html(client.as_ref(), category.url.as_str(), 5).await
        .unwrap_or_else(|e| panic!("Too many failed attempts: {e}"));
    let pages_nb = _get_page_nb(&doc);

    let futures = (1..pages_nb+1).map(|i| format!("{}/p/{i}", category.url))
        .map(|page| {
            _get_threads(client.clone(), page, site.clone())
        });

    let threads = tokio_stream::iter(futures).buffer_unordered(max_threads).collect::<Vec<_>>().await
        .into_iter().flatten().collect::<Vec<_>>();

    println!("Threads found: {}", threads.len());

    let futures = threads.into_iter().map(|thread| _get_messages(client.clone(), thread));

    let threads = tokio_stream::iter(futures).buffer_unordered(max_threads).collect::<Vec<_>>().await;

    category.threads = threads;

    if category.threads.len() as i32 != category.threads_nb {
        eprintln!("[WARNING] Number of threads found doesn't match number of threads announced by Wikidot.")
    }

    category
}


pub async fn forum_dl(data: Cli) {
    let client = Arc::new(reqwest::Client::new());
    let url = data.site.unwrap();
    let forum_dl_parameters = match data.script {
        Script::ForumDl(e) => e,
        _ => panic!() /* Impossible, treated in main */
    };
    let forum_path = url.clone() + forum_dl_parameters.forum_path.as_str() + if forum_dl_parameters.hidden { "/hidden/show" } else { "" };

    if data.verbose {
        eprintln!("Warning: --verbose has no effect for this script.");
    }

    println!("Downloading {forum_path}");

    let doc = download_html(&client, forum_path.as_str(), 5).await
        .unwrap_or_else(|e| panic!("Too many failed attempts: {e}"));

    let sel_group = Selector::parse("div.forum-group").unwrap();
    let sel_tr = Selector::parse("tr").unwrap();
    let sel_title = Selector::parse("td.name div.title a").unwrap();
    let sel_threads = Selector::parse(".threads").unwrap();
    let sel_posts = Selector::parse(".posts").unwrap();
    let groups = doc.select(&sel_group);
    let categories: Vec<_> = groups.map(|group| {
        group.select(&sel_tr)
            .skip(1)
            .map(|tr| Category {
                name: tr.select(&sel_title).next()
                    .and_then(|title| Some(title.inner_html()))
                    .expect(format!("Can't find title for a category: {}", tr.inner_html()).as_str()),
                url: url.clone() + tr.select(&sel_title).next()
                    .and_then(|title| title.attr("href"))
                    .expect(format!("Can't find title for a category: {}", tr.inner_html()).as_str())
                        .strip_prefix("/").expect("Category URL is not relative (but it should be).")
                    .rsplit_once('/').unwrap().0,
                threads_nb: tr.select(&sel_threads).next()
                    .and_then(|threads| threads.inner_html().parse().ok())
                    .unwrap_or(-1),
                posts: tr.select(&sel_posts).next()
                    .and_then(|posts| posts.inner_html().parse().ok())
                    .unwrap_or(-1),
                threads: Vec::new()
            })
    }).flatten().collect();

    println!("Categories found: {}", categories.len());

    let futures = categories.into_iter().map(|category| _category_dl(client.clone(), category, url.clone(), data.threads)).collect::<Vec<_>>();

    let categories = tokio_stream::iter(futures).buffer_unordered(1).collect::<Vec<_>>().await;

    let path = data.output.path().clone();

    match data.output_format {
        OutputFormat::JSON => {
            serde_json::to_writer_pretty(data.output, &categories)
                .unwrap_or_else(|e| panic!("Error writing into output file: {e}"));
        }
        OutputFormat::YAML => {
            serde_yaml::to_writer(data.output, &categories)
                .unwrap_or_else(|e| panic!("Error writing into output file: {e}"));
        }
    }

    println!("Results written in file {}", path);

}