//! The Raycast Store: search it, install the bundle it already built, and manage what is here.

use std::path::Path;
use std::time::Duration;

use rune::{Latest, Waker};
use serde_json::Value;

use crate::app::{App, Built, Tone};
use crate::ext::{ExtAct, net};
use crate::icons;
use crate::model::*;
use crate::util;

#[derive(Clone, Debug, Default)]
pub struct Listing {
    pub name: String,
    pub title: String,
    pub description: String,
    pub author: String,
    pub owner: String,
    pub downloads: u64,
    pub icon: String,
    pub download_url: String,
    pub store_url: String,
    pub commands: Vec<String>,
    pub categories: Vec<String>,
}

impl Listing {
    /// The directory an install lands in.
    pub fn id(&self) -> String {
        format!("{}.{}", self.owner, self.name)
    }
}

#[derive(Default)]
pub struct Store {
    results: Latest<(String, Vec<Listing>)>,
    pub listings: Vec<Listing>,
    answered: Option<String>,
    requested: Option<String>,
    installs: Latest<Result<String, String>>,
    pub installing: Option<String>,
}

fn parse(body: &str) -> Vec<Listing> {
    let Ok(json) = serde_json::from_str::<Value>(body) else { return Vec::new() };
    let s = |v: &Value, k: &str| v.get(k).and_then(Value::as_str).unwrap_or("").to_string();
    json.get("data").and_then(Value::as_array).map(|data| data.iter().map(|e| Listing {
        name: s(e, "name"),
        title: s(e, "title"),
        description: s(e, "description"),
        author: e.get("author").map(|a| s(a, "name")).unwrap_or_default(),
        owner: e.get("owner").map(|a| s(a, "handle")).filter(|h| !h.is_empty()).or_else(|| e.get("author").map(|a| s(a, "handle"))).unwrap_or_default(),
        downloads: e.get("download_count").and_then(Value::as_u64).unwrap_or(0),
        icon: e.get("icons").map(|i| { let d = s(i, "dark"); if d.is_empty() { s(i, "light") } else { d } }).unwrap_or_default(),
        download_url: s(e, "download_url"),
        store_url: s(e, "store_url"),
        commands: e.get("commands").and_then(Value::as_array).map(|c| c.iter().map(|c| s(c, "title")).collect()).unwrap_or_default(),
        categories: e.get("categories").and_then(Value::as_array).map(|c| c.iter().filter_map(Value::as_str).map(String::from).collect()).unwrap_or_default(),
    }).filter(|l| !l.name.is_empty() && !l.download_url.is_empty()).collect()).unwrap_or_default()
}

fn encode(s: &str) -> String {
    s.bytes().map(|b| if b.is_ascii_alphanumeric() { (b as char).to_string() } else { format!("%{b:02X}") }).collect()
}

fn fetch(query: &str) -> Vec<Listing> {
    // With nothing typed the store's own featured shelf leads, most installed first; an empty
    // search would return listings in no useful order.
    if query.trim().is_empty() {
        let mut featured = net::get("https://www.raycast.com/frontend_api/extensions/featured?per_page=40").map(|body| parse(&body)).unwrap_or_default();
        if !featured.is_empty() {
            featured.sort_by(|a, b| b.downloads.cmp(&a.downloads));
            return featured;
        }
    }
    let url = format!("https://www.raycast.com/frontend_api/extensions/search?q={}&per_page=40", encode(query));
    net::get(&url).map(|body| parse(&body)).unwrap_or_default()
}

impl Store {
    pub fn configure(&mut self, waker: Waker) {
        self.results = Latest::default().debounce(Duration::from_millis(220));
        self.results.set_waker(waker.clone());
        self.installs = Latest::default();
        self.installs.set_waker(waker);
    }

    fn request(&mut self, query: &str) {
        let q = query.trim().to_string();
        if self.requested.as_deref() == Some(q.as_str()) {
            return;
        }
        self.requested = Some(q.clone());
        self.results.request(q, |q| {
            let listings = fetch(&q);
            (q, listings)
        });
    }

    fn pending(&self) -> bool {
        self.requested != self.answered
    }
}

fn count(n: u64) -> String {
    if n >= 1_000_000 { format!("{:.1}M", n as f64 / 1e6) } else if n >= 1000 { format!("{:.1}k", n as f64 / 1e3) } else { n.to_string() }
}

pub fn build(app: &mut App, query: &str) -> Built {
    app.ext.store.request(query);
    let store = &app.ext.store;
    let items: Vec<Item> = store.listings.iter().map(|l| {
        let installed = app.ext.get(&l.id()).is_some();
        let icon = if l.icon.is_empty() { IconSrc::Glyph(&icons::PUZZLE) } else { net::cached(&l.icon).map(IconSrc::File).unwrap_or(IconSrc::Glyph(&icons::PUZZLE)) };
        let mut detail = Detail { heading: Some("Information".into()), ..Default::default() };
        let mut text = l.description.clone();
        if !l.commands.is_empty() {
            text.push_str("\n\nCommands\n");
            for c in l.commands.iter().take(14) {
                text.push_str(&format!("  •  {c}\n"));
            }
        }
        detail.markdown = Some(format!("## {}\n\n{}", l.title, text.replace("\n  •  ", "\n- ").replace("\n\nCommands\n", "\n\n**Commands**\n")));
        detail.fields.push(("Author".into(), l.author.clone()));
        detail.fields.push(("Downloads".into(), count(l.downloads)));
        if !l.categories.is_empty() {
            detail.fields.push(("Categories".into(), l.categories.join(", ")));
        }
        detail.fields.push(("Status".into(), if installed { "Installed".into() } else { "Not installed".into() }));
        let busy = store.installing.as_deref() == Some(l.id().as_str());
        let mut item = Item::new(format!("store:{}", l.id()), l.title.clone(), icon).subtitle(l.author.clone()).detail(detail);
        item.accessories.push(Accessory { icon: None, text: if busy { "Installing…".into() } else if installed { "Installed".into() } else { count(l.downloads) }, tag: None });
        item = if installed {
            item.action(Action::new("Reinstall Extension", &icons::DOWNLOAD, Act::Ext(ExtAct::Install(Box::new(l.clone())))))
                .action(Action::new("Uninstall Extension", &icons::TRASH, Act::Ext(ExtAct::Uninstall(l.id()))).destructive())
        } else {
            item.action(Action::new("Install Extension", &icons::DOWNLOAD, Act::Ext(ExtAct::Install(Box::new(l.clone())))))
        };
        item.action(Action::new("Open in Browser", &icons::GLOBE, Act::OpenUrl(l.store_url.clone())).keys("ctrl+o"))
    }).collect();
    let mut sections = Sections::default();
    sections.push(if query.trim().is_empty() { "Featured" } else { "Results" }, items);
    Built::list(sections, "Search the Raycast Store...").layout(Layout::Split).loading(store.pending()).keep()
        .empty(if store.pending() { "Loading the Store…" } else { "No Extensions Found" }, if store.pending() { "" } else { "Check the connection, or try another search." })
}

pub fn installed(app: &App, query: &str) -> Built {
    let q = query.trim();
    let mut sections = Sections::default();
    let items: Vec<Item> = app.ext.installed.iter()
        .filter(|e| q.is_empty() || crate::features::fuzzy::score_item(q, &e.title, &e.description).is_some())
        .map(|e| {
            let icon = e.icon.clone().map(IconSrc::File).unwrap_or(IconSrc::Glyph(&icons::PUZZLE));
            let commands = e.commands.len();
            Item::new(format!("installed:{}", e.id), e.title.clone(), icon).subtitle(e.author.clone())
                .trailing(&format!("{commands} command{}", if commands == 1 { "" } else { "s" }))
                .action(Action::new("Configure Extension", &icons::GEAR, Act::Ext(ExtAct::Configure(e.id.clone()))))
                .action(Action::new("Show in File Manager", &icons::FOLDER, Act::Reveal(e.dir.join("package.json"))).keys("ctrl+enter"))
                .action(Action::new("Open Log", &icons::FILE_TEXT, Act::OpenPath(util::data_dir().join("logs").join(format!("{}.log", e.id)))).keys("ctrl+l"))
                .action(Action::new("Uninstall Extension", &icons::TRASH, Act::Confirm(Box::new(Confirm {
                    title: format!("Uninstall {}?", e.title), message: "Its commands and saved preferences are removed.".into(),
                    icon: &icons::TRASH, confirm: "Uninstall".into(), destructive: true, act: Act::Ext(ExtAct::Uninstall(e.id.clone())),
                }))).keys("ctrl+x").destructive().section(""))
        }).collect();
    sections.push("Installed", items);
    sections.push("Get More", vec![
        Item::new("installed:store", "Browse the Store", IconSrc::Badge(&icons::STORE, crate::theme::Tint::Cyan)).trailing("Command")
            .action(Action::new("Open Store", &icons::STORE, Act::Push(Route::Store))),
        Item::new("installed:demo", "Install Demo Extension", IconSrc::Badge(&icons::BOX, crate::theme::Tint::Green)).trailing("Command")
            .action(Action::new("Install", &icons::DOWNLOAD, Act::Ext(ExtAct::InstallDemo))),
    ]);
    Built::list(sections, "Search installed extensions...").keep()
}

/// Copies what an extension needs to run: its manifest, built commands, assets and any modules
/// its build left outside the bundle.
fn copy_built(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let (path, name) = (entry.path(), entry.file_name());
        let target = to.join(&name);
        if path.is_dir() {
            copy_built(&path, &target)?;
        } else {
            std::fs::copy(&path, &target)?;
        }
    }
    Ok(())
}

fn download(listing: &Listing) -> Result<String, String> {
    let work = util::runtime_dir().join(format!("echospot-install-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&work);
    std::fs::create_dir_all(&work).map_err(|e| e.to_string())?;
    let zip = work.join("extension.zip");
    let fetched = std::process::Command::new("curl").args(["-sL", "--max-time", "120", "-o"]).arg(&zip).arg(&listing.download_url)
        .status().is_ok_and(|s| s.success());
    if !fetched {
        return Err("The download failed.".into());
    }
    let unpacked = work.join("unpacked");
    let ok = std::process::Command::new("unzip").arg("-qo").arg(&zip).arg("-d").arg(&unpacked).status().is_ok_and(|s| s.success())
        || std::process::Command::new("bsdtar").arg("-xf").arg(&zip).arg("-C").arg(&unpacked).status().is_ok_and(|s| s.success());
    if !ok {
        return Err("The archive could not be unpacked (is unzip installed?).".into());
    }
    // The bundle sits at the archive's root or one folder down.
    let root = if unpacked.join("package.json").is_file() { unpacked.clone() } else {
        std::fs::read_dir(&unpacked).map_err(|e| e.to_string())?.flatten().map(|e| e.path())
            .find(|p| p.join("package.json").is_file()).ok_or("The archive holds no extension.")?
    };
    let target = super::extensions_dir().join(listing.id());
    let _ = std::fs::remove_dir_all(&target);
    copy_built(&root, &target).map_err(|e| e.to_string())?;
    let _ = std::fs::remove_dir_all(&work);
    Ok(listing.title.clone())
}

/// `echospot ext search <query>` and `echospot ext install <query>`: the store from a terminal.
pub fn cli(args: &[String]) {
    let query = args.get(1..).map(|a| a.join(" ")).unwrap_or_default();
    let listings = fetch(&query);
    match args.first().map(String::as_str) {
        Some("search") => {
            for l in &listings {
                println!("{:<34} {:<22} {:>8}  {}", l.title, l.id(), count(l.downloads), util::truncate(&l.description, 60));
            }
        }
        Some("install") => match listings.first() {
            Some(l) => match download(l) {
                Ok(title) => println!("Installed {title} to {}", super::extensions_dir().join(l.id()).display()),
                Err(e) => eprintln!("Install failed: {e}"),
            },
            None => eprintln!("No extension matches “{query}”."),
        },
        _ => eprintln!("usage: echospot ext <search|install> <query>"),
    }
}

pub fn install(app: &mut App, listing: Listing) {
    if app.ext.store.installing.is_some() {
        return app.toast("An install is already running", Tone::Neutral);
    }
    app.ext.store.installing = Some(listing.id());
    app.toast_progress(format!("Installing {}…", listing.title));
    app.ext.store.installs.request(listing, |l| download(&l));
    app.refresh_keeping();
}

pub fn install_demo(app: &mut App) {
    let source = std::env::current_exe().ok().and_then(|exe| {
        exe.ancestors().map(|a| a.join("runtime/fixtures/demo")).find(|p| p.join("package.json").is_file())
    });
    match source {
        Some(source) => match copy_built(&source, &super::extensions_dir().join("echospot.demo")) {
            Ok(()) => {
                app.ext.rescan();
                app.refresh();
                app.toast("Demo Extension Installed", Tone::Success);
            }
            Err(e) => app.toast(format!("Install failed: {e}"), Tone::Danger),
        },
        None => app.toast("The demo ships with the source tree only", Tone::Neutral),
    }
}

pub fn poll(app: &mut App) -> bool {
    let mut changed = false;
    if let Some((query, listings)) = app.ext.store.results.take() {
        app.ext.store.answered = Some(query);
        app.ext.store.listings = listings;
        changed = true;
    }
    if let Some(result) = app.ext.store.installs.take() {
        app.ext.store.installing = None;
        app.ext.rescan();
        match result {
            Ok(title) => app.toast(format!("{title} Installed"), Tone::Success),
            Err(error) => app.toast(error, Tone::Danger),
        }
        changed = true;
    }
    if changed && matches!(app.top().route, Route::Store | Route::Extensions) {
        app.refresh_keeping();
    }
    changed
}
