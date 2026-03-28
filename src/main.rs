use watcher_lib::{
    config::{WatchItem, YamlChoice},
    watcher::WatchFiles,
};

fn main() {
    let item = WatchItem {
        name: "Hello".to_string(),
        watch: YamlChoice::Single("src/*".to_string()),
        run: YamlChoice::Single("echo hello\necho goodbye".to_string()),
        ignore: None,
    };

    if let Err(e) = WatchFiles::start(vec![item]) {
        eprintln!("{}", e);
    };
}
