use watcher_lib::{
    config::{EventFlags, WatchItem, YamlChoice},
    watcher::WatchFiles,
};

fn main() {
    let item = WatchItem {
        name: "Hello".to_string(),
        watch: YamlChoice::Single("src/*".to_string()),
        run: YamlChoice::Single("echo hello\necho goodbye".to_string()),
        ignore: None,
        events: EventFlags::MODIFY,
        debounce: 100,
    };

    if let Err(e) = WatchFiles::default().start(vec![item]) {
        eprintln!("{}", e);
    };
}
