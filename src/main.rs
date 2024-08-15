use clap::Parser;
use minijinja::{context, Environment};
use plotly::{
    common::{Orientation, Title},
    layout::{BarMode, Margin},
    Bar, Layout, Plot,
};
use polars::prelude::*;
use std::path::PathBuf;
use std::process::Command;
use tokei::{Config, Languages};

static TEMPLATE: &str = include_str!("../templates/index.html");
const PLOT_WIDTH: usize = 1200;

#[derive(Parser)]
#[command(version, about, long_about = None)]
struct Cli {
    /// Path to a git repository
    path: PathBuf,
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

fn get_commit_log(path: &PathBuf) -> DataFrame {
    let output = Command::new("git")
        .arg("log")
        .arg("--date=short")
        .arg("--pretty=format:%ad,%an")
        .current_dir(path)
        .output()
        .expect("Failed to execute command");
    let result = String::from_utf8(output.stdout).expect("Unable to parse Command output");
    let df = df![
        "log" => result.lines().collect::<Vec<&str>>()
    ]
    .unwrap();

    let dt_options = StrptimeOptions {
        format: Some("%Y-%m-%d".into()),
        strict: false,
        ..Default::default()
    };

    // split lines by comma into multiple columns
    df
        .lazy()
        .with_columns([
            col("log")
                .str()
                .split(lit(","))
                .list()
                .get(lit(0), false)
                .str()
                .strptime(DataType::Date, dt_options, Default::default())
                .alias("date"),
            col("log")
                .str()
                .split(lit(","))
                .list()
                .get(lit(1), false)
                .alias("author"),
        ])
        .select([col("date"), col("author")])
        .collect()
        .unwrap()
}

fn plot_commit_history(df: &DataFrame) -> Plot {
    let mut plot = Plot::new();

    // do a groupby count per date and author for the dataframe
    let commits_per_day_per_author = df
        .clone()
        .lazy()
        .group_by([col("date").dt().strftime("%Y-%m"), col("author")])
        .agg([col("author").count().alias("count")])
        .sort(["date"], Default::default())
        .collect()
        .unwrap();

    let author_dfs =
        DataFrame::partition_by(&commits_per_day_per_author, ["author"], true).unwrap();
    for author_df in author_dfs {
        let x: Vec<String> = author_df
            .column("date")
            .unwrap()
            .str()
            .unwrap()
            .into_iter()
            .map(|date| date.unwrap().into())
            .collect();

        let y = author_df
            .column("count")
            .unwrap()
            .u32()
            .unwrap()
            .into_iter()
            .map(|count| count.unwrap())
            .collect();

        let author: String = author_df["author"].str().unwrap().get(0).unwrap().into();
        let trace = Bar::new(x, y).name(author);
        plot.add_trace(trace);
    }

    let layout = Layout::new()
        .width(PLOT_WIDTH)
        .bar_mode(BarMode::Stack)
        // .x_axis(Axis::new().range(date_range))
        .title(Title::from("Commit activity per author"));
    plot.set_layout(layout);

    plot
}

fn plot_commit_count_per_author(df: &DataFrame, top: usize) -> Plot {
    let mut plot = Plot::new();

    // do a groupby author count
    let commits_per_author = df
        .clone()
        .lazy()
        .group_by(["author"])
        .agg([col("date").count().alias("count")])
        .sort(["count"], Default::default())
        .collect()
        .unwrap()
        .tail(Some(top));

    let x: Vec<u32> = commits_per_author["count"]
        .u32()
        .unwrap()
        .into_iter()
        .map(|count| count.unwrap())
        .collect();
    // let x be a vec of the counts from the commits_per_author dataframe
    let y: Vec<String> = commits_per_author["author"]
        .str()
        .unwrap()
        .into_iter()
        .map(|count| count.unwrap().into())
        .collect();

    let trace = Bar::new(x, y).orientation(Orientation::Horizontal);
    plot.add_trace(trace);
    let layout = Layout::new()
        .width(PLOT_WIDTH / 2)
        .title(Title::from("Commits per author"))
        .margin(Margin::new().left(200).right(200));
    plot.set_layout(layout);

    plot
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

    let log = get_commit_log(&cli.path);
    let activity_plot = plot_commit_history(&log);
    let commits_per_author_plot = plot_commit_count_per_author(&log, 10);

    let languages = get_repo_languages(&cli.path);

    // Render the template to html
    let mut env = Environment::new();
    env.add_template("index.html", TEMPLATE).unwrap();
    let template = env.get_template("index.html").unwrap();
    let ctx = context! {
    activity_plot => activity_plot.to_inline_html(None),
    commits_per_author_plot => commits_per_author_plot.to_inline_html(None),
     languages => languages
    };
    let result = template.render(ctx).unwrap();

    // Write to file
    let filename = "git-report.html";
    std::fs::write(filename, result).unwrap();

    if webbrowser::open(filename).is_ok() {
        println!("Done!");
    }
}
