use chrono::NaiveDate;
use clap::Parser;
use minijinja::{context, Environment, Value};
use plotly::{
    common::{Orientation, Title},
    layout::{BarMode, Margin},
    Bar, Layout, Plot,
};
use std::process::Command;
use std::{cmp::min, hash::Hash};
use std::{collections::HashMap, path::PathBuf};
use tokei::{Config, Languages};

static TEMPLATE: &str = include_str!("../templates/index.html");
const PLOT_WIDTH: usize = 1200;

#[derive(Parser)]
#[command(version, about, long_about = None)]
struct Cli {
    /// Path to a git repository
    path: PathBuf,
}

#[derive(Eq, Hash, PartialEq, Debug)]
struct Commit {
    date: NaiveDate,
    author: String,
}

fn get_repo_languages(repo_path: &PathBuf) -> Vec<(tokei::LanguageType, tokei::Language)> {
    let mut languages = Languages::new();
    let paths = &[repo_path];

    // Exclude any path that contains any of these strings.
    let excluded = &[];

    // `Config` allows you to configure what is searched and counted.
    let config = Config::default();

    languages.get_statistics(paths, excluded, &config);

    // sort languages by total number of lines
    let mut languages = languages.into_iter().collect::<Vec<_>>();
    languages.sort_by(|a, b| b.1.lines().cmp(&a.1.lines()));
    languages
}

fn get_commit_log(path: &PathBuf) -> Vec<Commit> {
    let output = Command::new("git")
        .arg("log")
        .arg("--format=%as,%cn")
        .current_dir(path)
        .output()
        .expect("Failed to execute command");
    let result = String::from_utf8(output.stdout).expect("Unable to parse git command output");

    // split the results into a vec of tuples
    result
        .lines()
        .map(|line| {
            let parts: Vec<&str> = line.split(',').collect();
            Commit {
                date: NaiveDate::parse_from_str(parts[0], "%Y-%m-%d")
                    .expect("Could not parse value as a date"),
                author: parts[1].to_string(),
            }
        })
        .collect()
}

fn plot_commit_history(commits: &Vec<Commit>) -> Plot {
    let mut plot = Plot::new();

    // do a groupby count per date and author for the commits
    // count commits per author using plain vec methods
    let mut count: HashMap<String, HashMap<String, i32>> = HashMap::new();
    for commit in commits {
        *count
            .entry(commit.author.clone())
            .or_default()
            .entry(commit.date.clone().format("%Y-%m").to_string())
            .or_insert(0) += 1;
    }

    // let mut count_vec: Vec<(String, Vec<NaiveDate>)> = count.into_iter().collect();
    for (author, counts) in count.into_iter() {
        let x: Vec<String> = counts.clone().keys().map(|x| x.to_string()).collect();
        let y: Vec<i32> = counts.clone().values().map(|x| x.to_owned()).collect();
        let trace = Bar::new(x, y).name(author);
        plot.add_trace(trace);
    }

    let layout = Layout::new()
        .width(PLOT_WIDTH - 50) // make the legend fit in the containing div
        .bar_mode(BarMode::Stack)
        // .x_axis(Axis::new().range(date_range))
        .title(Title::from("Commit activity per author"));
    plot.set_layout(layout);

    plot
}

fn plot_commit_count_per_author(commits: &Vec<Commit>, n: usize) -> Plot {
    let mut plot = Plot::new();

    // count commits per author using plain vec methods
    let mut count: HashMap<String, u32> = HashMap::new();
    for commit in commits {
        *count.entry(commit.author.clone()).or_insert(0) += 1;
    }

    // sort counts
    let mut count_vec: Vec<(String, u32)> = count.into_iter().collect();
    count_vec.sort_by_key(|&(_, count)| count);

    // get top n items
    let tail: usize = count_vec.len() - min(count_vec.len(), n);
    let top_n = count_vec[tail..].to_vec();

    let y: Vec<String> = top_n
        .clone()
        .into_iter()
        .map(|(author, _)| author)
        .collect();
    let x: Vec<u32> = top_n.clone().into_iter().map(|(_, count)| count).collect();

    let trace = Bar::new(x, y).orientation(Orientation::Horizontal);
    plot.add_trace(trace);
    let layout = Layout::new()
        .width(PLOT_WIDTH / 2)
        .title(Title::from("Commits per author"))
        .margin(Margin::new().left(200).right(200));
    plot.set_layout(layout);

    plot
}

fn render_template(ctx: Value) -> String {
    let mut env = Environment::new();
    env.add_template("index.html", TEMPLATE).unwrap();
    let template = env.get_template("index.html").unwrap();

    template.render(ctx).unwrap()
}

fn main() {
    let cli = Cli::parse();

    // Check if path exists, if not, error
    if !cli.path.exists() {
        eprintln!("Error: Path does not exist: {:?}", cli.path);
        std::process::exit(1);
    }

    // check if path is a valid git repository
    if !cli.path.join(".git").exists() {
        eprintln!(
            "Error: Path is not a git repository. Expected a '.git' directory at {:?}/.git",
            cli.path
        );
        std::process::exit(1);
    }

    let commits = get_commit_log(&cli.path);
    let activity_plot = plot_commit_history(&commits);
    let commits_per_author_plot = plot_commit_count_per_author(&commits, 10);

    let languages = get_repo_languages(&cli.path);

    let ctx = context! {
    path => cli.path,
    activity_plot => activity_plot.to_inline_html(None),
    commits_per_author_plot => commits_per_author_plot.to_inline_html(None),
    languages => languages
    };

    let template = render_template(ctx);

    // Write to file
    let filename = "git-report.html";
    std::fs::write(filename, template).unwrap();

    if webbrowser::open(filename).is_ok() {
        println!("Done!");
    }
}
