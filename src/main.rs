use fs::File;
use regex::Regex;
use std::{collections::HashSet, fs, io::Write};
use structopt::StructOpt;
use walkdir::{DirEntry, WalkDir};

mod cli;

#[derive(Debug, Clone)]
struct MigrationFile {
    dir: DirEntry,
    app: String,
    filename: String,
}

impl MigrationFile {
    fn new(dir: DirEntry, app: String, filename: String) -> Self {
        Self { dir, app, filename }
    }
}
const APP_FIXTURE: &str = r#"""#;

fn main() {
    let matches = cli::Opt::from_args();
    let base_path = matches.config.clone();

    let re = Regex::new(
        r###"(?x)
    (?P<app>"\w+")  # the year
    ,\s
    (?P<migration>"\w+") # the month
    "###,
    )
    .unwrap();

    let base_path = base_path.to_str().unwrap();

    let filter_path = |item: &DirEntry| {
        item.path().to_str().unwrap_or("").contains("migrations")
            && item
                .file_name()
                .to_str()
                .map(|f| f.ends_with("py") && !f.starts_with("__init__"))
                .unwrap_or(false)
    };

    let mut migration_files: Vec<_> = WalkDir::new(matches.config.clone())
        .into_iter()
        .filter_map(|f| f.ok())
        .filter(|f| filter_path(&f))
        .filter_map(|f| {
            let parent = f.path().parent().unwrap().parent().unwrap();
            let app = parent.file_name().unwrap().to_str().unwrap().to_owned();
            let filename = f
                .file_name()
                .to_str()
                .unwrap()
                .strip_suffix(".py")
                .unwrap()
                .to_owned();
            Some(MigrationFile::new(f, app, filename))
        })
        .collect();
    migration_files.sort_by(|a, b| a.filename.partial_cmp(&b.filename).unwrap());

    let mut res: Vec<_> = migration_files
        .into_iter()
        .map(|e| {
            let contents = fs::read_to_string(e.dir.path()).unwrap();
            // Extract the migration dependencies from the contents

            let mut deps = false;
            let deps_lines: Vec<_> = contents
                .split("\n")
                .filter(|line| {
                    if !deps {
                        if line.contains("dependencies = [") {
                            deps = true;
                        }
                    }
                    if deps {
                        if line.contains("]") {
                            deps = false;
                            return true;
                        }
                    }
                    deps
                })
                .map(|e| e.to_owned())
                .collect();

            let mut dependencies = vec![];
            for line in deps_lines {
                let caps = re.captures(&line);
                match caps {
                    Some(caps) => {
                        let app = caps["app"]
                            .strip_prefix(APP_FIXTURE)
                            .and_then(|e| e.strip_suffix(APP_FIXTURE))
                            .unwrap()
                            .to_owned();
                        let migration = caps["migration"]
                            .strip_prefix(APP_FIXTURE)
                            .and_then(|e| e.strip_suffix(APP_FIXTURE))
                            .unwrap()
                            .to_owned();
                        dependencies.push((app, migration));
                    }
                    None => {}
                }
            }
            (e, dependencies)
        })
        .collect();

    // Generate mermaid diagrams
    res.sort_by(|a, b| {
        let comp = a.0.app.partial_cmp(&b.0.app).unwrap();
        // a.0.filename.as_str().partial_cmp(b.0.filename.as_str());
        comp
    });
    let mut res_iter = res.into_iter();
    let first = res_iter.next();

    match first {
        Some(first) => {
            let mut file = File::create("output.md").unwrap();
            file.write(
                format!(
                    r###"
```mermaid
flowchart TB
subgraph {:}
{:}.{:}
        "###,
                    &first.0.app, &first.0.app, &first.0.filename
                )
                .as_bytes(),
            )
            .unwrap();
            let mut prev_app = first.0.app;
            let mut previous_migration = None;
            for (migration, dependencies) in res_iter {
                if prev_app != migration.app {
                    file.write(b"end").unwrap();
                    let to_write = format!(
                        r###"
subgraph {:}
{:}.{:}
"###,
                        &migration.app, &migration.app, &migration.filename
                    );
                    prev_app = migration.app.clone();
                    previous_migration = None;
                    file.write(to_write.as_bytes()).unwrap();
                }
                // write the dependencies
                let current = format!("{}.{}", &migration.app, migration.filename);

                let dependency_vec: Vec<_> = dependencies
                    .iter()
                    .map(|(app, migr)| {
                        let dep = format!("{}.{}", app, migr);
                        format!("{} --> {}\n", dep, current)
                    })
                    .collect();

                match previous_migration {
                    Some(prev) => {
                        let formatted_string = format!("{} --> {}\n", prev, current);
                        if !dependency_vec.contains(&formatted_string) {
                            file.write(formatted_string.as_bytes()).unwrap();
                        }
                    }
                    None => {}
                }
                println!("{:#?}", &dependency_vec.join(""));
                let bytes = dependency_vec.join("");
                let bytes = bytes.as_bytes();
                file.write(bytes).unwrap();
                previous_migration = Some(current);
            }
            file.write(b"end").unwrap();
            file.write(
                br###"
```
"###,
            )
            .unwrap();
        }
        None => { /* Nothing to do here */ }
    }
}
