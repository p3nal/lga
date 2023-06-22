mod ui;
use confy;
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use file_format::{FileFormat, Kind};
use humansize::{format_size, DECIMAL};
use serde::{Deserialize, Serialize};
use std::{
    cmp::Ordering,
    env,
    fs::{
        self, copy, create_dir, create_dir_all, read_to_string, remove_dir, remove_dir_all,
        remove_file, rename, File,
    },
    io::{self, BufRead, BufReader, Seek, Write},
    mem,
    os::unix::prelude::MetadataExt,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, Instant, SystemTime},
    usize,
};
use tui::{
    backend::{Backend, CrosstermBackend},
    widgets::ListState,
    Terminal,
};

// fuck it. im adding a whole lot of other shit here
#[derive(PartialEq, Eq, PartialOrd, Ord)]
struct Item<T, U> {
    path: T,
    tagged: bool,
    preview: Option<U>,
}
impl<T, U> Item<T, U> {
    fn new(t: T, tagged: bool) -> Item<T, U> {
        Item {
            path: t,
            tagged,
            preview: None,
        }
    }
    fn tag(&mut self) {
        self.tagged = true
    }
    fn toggle_tagged(&mut self) {
        self.tagged = !self.tagged
    }
    fn set_preview(&mut self, preview: U) {
        self.preview = Some(preview)
    }
    fn set_item(&mut self, item: T) {
        self.path = item
    }
}

#[derive(Clone)]
struct StatefulList<T> {
    items: Vec<T>,
    state: ListState,
}
impl<T> StatefulList<T> {
    fn next(&mut self) {
        if self.items.len() > 0 {
            let i = match self.state.selected() {
                Some(i) => {
                    if i >= self.items.len() - 1 {
                        0
                    } else {
                        i + 1
                    }
                }
                None => 0,
            };
            self.state.select(Some(i));
        }
    }

    fn prev(&mut self) {
        if self.items.len() > 0 {
            let i = match self.state.selected() {
                Some(i) => {
                    if i == 0 {
                        self.items.len() - 1
                    } else {
                        i - 1
                    }
                }
                None => 0,
            };
            self.state.select(Some(i));
        }
    }
}

enum Confirm {
    DeleteFolder,
}

enum InputMode {
    // normal mode for navigation and all
    Normal,
    // command mode for issuing short commands, navigation still works
    Command(String),
    // input mode: navigation doesnt work, all input gets buffered until enter
    // or esc is clicked
    Input(String),
    // confirmation mode for y/n confirmations, call it with the input already
    // filled
    Confirmation(Confirm),
}
impl InputMode {
    fn push(&mut self, c: char) {
        match self {
            InputMode::Command(s) | InputMode::Input(s) => s.push(c),
            _ => {}
        }
    }
    fn pop(&mut self) -> Option<char> {
        match self {
            InputMode::Command(s) | InputMode::Input(s) => s.pop(),
            _ => None,
        }
    }
    fn get_str(&self) -> String {
        match self {
            InputMode::Command(s) | InputMode::Input(s) => s.to_string(),
            _ => String::new(),
        }
    }
}

enum ListOrder {
    Default,
    Name,
    NameReverse,
    Modified,
    ModifiedReverse,
    Created,
    CreatedReverse,
    DirsFirst,
    FilesFirst,
}

#[derive(Serialize, Deserialize)]
struct Config {
    tags: Vec<PathBuf>,
}
impl ::std::default::Default for Config {
    fn default() -> Self {
        Self { tags: vec![] }
    }
}

pub struct App {
    left_column: StatefulList<Item<PathBuf, String>>,
    middle_column: StatefulList<Item<PathBuf, String>>,
    right_column: StatefulList<Item<PathBuf, String>>,
    orderby: ListOrder,
    pwd: PathBuf,
    hidden: bool,
    message: String,
    metadata: String,
    // Current value of the input command
    // Current input mode
    input_mode: InputMode,
    // register for yanking and moving
    register: PathBuf,
    // make this a hashmap ig
    config: Config,
}

impl App {
    fn new(pwd: PathBuf, hidden: bool) -> App {
        // we might need to display some message on start
        let message = String::new();
        let cfg: Config = confy::load("lga", Some("tags")).unwrap();
        // list the parent stuff
        let left_column_items = match pwd.parent() {
            Some(parent) => ls(parent, hidden, &ListOrder::DirsFirst, &cfg.tags),
            None => vec![],
        };
        // list pwd stuff
        let middle_column_items = ls(&pwd, hidden, &ListOrder::DirsFirst, &cfg.tags);
        // list child stuff
        let right_column_items = ls(
            &middle_column_items
                .get(0)
                .unwrap_or(&Item::new(PathBuf::default(), false))
                .path
                .as_path(),
            hidden,
            &ListOrder::DirsFirst,
            &cfg.tags,
        );
        App {
            left_column: StatefulList {
                items: left_column_items,
                state: ListState::default(),
            },
            middle_column: StatefulList {
                items: middle_column_items,
                state: ListState::default(),
            },
            right_column: StatefulList {
                items: right_column_items,
                state: ListState::default(),
            },
            orderby: ListOrder::DirsFirst,
            pwd: pwd.to_path_buf(),
            hidden,
            message,
            metadata: String::new(),
            input_mode: InputMode::Normal,
            register: PathBuf::new(),
            config: cfg,
        }
    }

    fn go_right(&mut self) {
        match self.get_selected() {
            Some(selected) => {
                let selected = &selected.path;
                if selected.is_dir() {
                    self.pwd = selected.to_path_buf();
                    // what a fucked up fix
                    self.left_column = mem::replace(
                        &mut self.middle_column,
                        mem::replace(
                            &mut self.right_column,
                            StatefulList {
                                items: vec![],
                                state: ListState::default(),
                            },
                        ),
                    );
                    self.refresh_right_column();
                } else if selected.is_file() {
                    // i should probably use kind
                    match FileFormat::from_file(selected).unwrap().kind() {
                        // TODO fix these, read the programs from a config file
                        Kind::Book | Kind::Document => {
                            Command::new("/usr/bin/zathura")
                                .arg(selected.as_path())
                                .stderr(Stdio::null())
                                .spawn()
                                .expect("failed to exec");
                        }
                        Kind::Text | Kind::Application => {
                            self.set_message("opening these messes up the terminal for now")
                            // Command::new("/usr/bin/nvim")
                            //     .arg(selected.as_path())
                            //     .stderr(Stdio::null())
                            //     .spawn()
                            //     .expect("failed to exec");
                        }
                        Kind::Image => {
                            Command::new("/usr/bin/sxiv")
                                .arg(selected.as_path())
                                .stderr(Stdio::null())
                                .spawn()
                                .expect("failed to exec");
                        }
                        Kind::Video => {
                            Command::new("/usr/bin/vlc")
                                .arg(selected.as_path())
                                .stderr(Stdio::null())
                                .spawn()
                                .expect("failed to exec");
                        }
                        _ => self.set_message("yeah i cant open this so far"),
                    }
                }
            }
            None => self.set_message("none selected"),
        }
    }

    fn go_left(&mut self) {
        // we have to somehow select the parent when going left
        match self.pwd.parent() {
            Some(parent) => {
                let parent_index: Option<usize> =
                    get_item_index(&self.pwd, &self.left_column.items);
                // again, i do not like the couple next lines.
                self.right_column = mem::replace(
                    &mut self.middle_column,
                    mem::replace(
                        &mut self.left_column,
                        StatefulList {
                            items: vec![],
                            state: ListState::default(),
                        },
                    ),
                );
                self.middle_column.state.select(parent_index);
                self.pwd = parent.to_path_buf();
                match self.pwd.parent() {
                    Some(parent) => self.left_column.items = self.ls(parent),
                    None => self.left_column.items = vec![],
                }
            }
            None => {}
        };
    }

    fn refresh_right_column(&mut self) {
        match self.get_selected() {
            Some(selected) => {
                let selected = &selected.path;
                let path = selected.as_path();
                if selected.is_dir() {
                    self.right_column.items = self.ls(&path);
                    if self.right_column.items.len() > 0 {
                        self.right_column.state.select(Some(0));
                    }
                } else if selected.is_file() {
                    self.right_column.items = vec![];
                } else {
                    // just cuz it probably needs to be handled later
                    self.right_column.items = vec![];
                }
            }
            None => {}
        }
    }

    fn refresh_left_column(&mut self) {
        match self.pwd.parent() {
            Some(parent) => self.left_column.items = self.ls(parent),
            None => self.left_column.items = vec![],
        };
    }

    fn refresh_middle_column(&mut self) {
        self.middle_column.items = self.ls(&self.pwd);
    }

    fn refresh_all(&mut self) {
        self.refresh_left_column();
        self.refresh_middle_column();
        self.refresh_right_column();
    }

    fn toggle_hidden_files(&mut self) {
        self.hidden = !self.hidden;
    }

    fn get_selected(&self) -> Option<&Item<PathBuf, String>> {
        self.middle_column
            .items
            .get(self.middle_column.state.selected().unwrap_or(0))
    }

    fn get_mut_selected(&mut self) -> Option<&mut Item<PathBuf, String>> {
        self.middle_column
            .items
            .get_mut(self.middle_column.state.selected().unwrap_or(0))
    }

    fn ls(&self, pwd: &Path) -> Vec<Item<PathBuf, String>> {
        ls(pwd, self.hidden, &self.orderby, &self.config.tags)
    }

    fn set_metadata(&mut self) {
        let size = match self.get_selected() {
            Some(selected) => match selected.path.metadata() {
                Ok(metadata) => format_size(metadata.size(), DECIMAL),
                Err(_) => String::new(),
            },
            None => String::new(),
        };
        let index = match self.middle_column.state.selected() {
            Some(index) => {
                let count = &self.middle_column.items.len();
                let index = index + 1;
                format!("{index}/{count} ")
            }
            None => String::new(),
        };
        self.metadata = format!("{size}  {index}")
    }

    fn set_message<T: AsRef<str>>(&mut self, message: T) {
        self.message = message.as_ref().to_string()
    }

    fn execute(&mut self) {
        let command = self.input_mode.get_str();
        match command.split_once(' ') {
            Some(command) => match command.0 {
                // then it has two words as expected
                ":rename" => {
                    let src = &self.get_selected().unwrap().path;
                    let dst = PathBuf::new().join(&self.pwd).join(command.1);
                    if src.eq(&dst) {
                        self.set_message("nothing to do")
                    } else {
                        match rename(src, dst) {
                            Ok(_) => {
                                self.set_message("renamed file");
                                self.refresh_middle_column();
                            }
                            Err(_) => {
                                self.set_message("something went wrong while renaming");
                            }
                        }
                    }
                }
                // todo implement selecting things once created
                ":touch" => {
                    let dst = PathBuf::new().join(&self.pwd).join(command.1);
                    if !Path::exists(&dst) {
                        match File::create(&dst) {
                            Ok(_) => {
                                self.set_message("file created");
                                self.refresh_middle_column();
                                let index = get_item_index(&dst, &self.middle_column.items);
                                self.middle_column.state.select(index);
                            }
                            Err(_) => self.set_message("error creating file"),
                        };
                    } else {
                        self.set_message("path already exists")
                    }
                }
                ":mkdir" => {
                    let dst = PathBuf::new().join(&self.pwd).join(command.1);
                    if !Path::exists(&dst) {
                        match create_dir(dst) {
                            Ok(_) => {
                                self.set_message("directory created");
                                self.refresh_middle_column();
                            }
                            Err(_) => self.set_message("error creating directory"),
                        };
                    } else {
                        self.set_message("path already exists")
                    }
                }
                ":find" => {
                    match self.inc_find() {
                        Some(_) => {}
                        None => self.middle_column.state.select(Some(0))
                    };
                    self.set_message("");
                    self.refresh_right_column()
                }
                _ => {
                    // make this into some easter egg, randomize statements and throw
                    // them in for a pinch of fun
                    self.set_message(
                        "i traveled the earth to find your command and couldnt find it",
                    )
                }
            },
            None => {
                // then it has only one word
                match command.as_str() {
                    ":q" | ":quit" => {
                        // implement quitting.. lol
                        self.set_message("press q to quit i have not implemented the command yet...")
                    }
                    _ => self.set_message("i have never seen this man in my entire life"),
                }
            }
        }
    }

    fn confirm(&mut self) {
        match self.input_mode {
            InputMode::Confirmation(Confirm::DeleteFolder) => match self.get_selected() {
                Some(selected) => {
                    // delete all
                    match remove_dir_all(selected.path.as_path()) {
                        Ok(_) => {
                            self.set_message("deleted!");
                            self.refresh_middle_column();
                        }
                        Err(_) => self.set_message("cant delete"),
                    };
                }
                None => self.set_message("Nothing is selected"),
            },
            _ => {}
        }
    }

    // a good thing to do is to make a trash folder and collect stuff to delete
    // there, then just before the app closes we can issue a delete...
    // although this may make exiting a bit slower i recon... anyway
    fn delete_file(&mut self) {
        // todo after deleting, select something else if dir isnt empty
        match self.get_selected() {
            Some(selected) => {
                let selected = &selected.path;
                let path = selected.as_path();
                if selected.is_dir() {
                    // check if empty
                    match selected.read_dir().unwrap().next().is_none() {
                        true => {
                            match remove_dir(path) {
                                Ok(_) => self.set_message("deleted empty dir"),
                                Err(_) => self.set_message("wont delete"),
                            };
                        }
                        false => {
                            // this sucks less ig
                            self.input_mode = InputMode::Confirmation(Confirm::DeleteFolder);
                            self.set_message("are you sure you want to delete this folder and all of its contents? [y/n]")
                        }
                    }
                } else if selected.is_file() {
                    match remove_file(path) {
                        Ok(_) => self.set_message("deleted file"),
                        Err(_) => self.set_message("wont delete"),
                    };
                } else {
                    self.set_message("this type of files hasn't been handled yet")
                }
                self.refresh_middle_column();
            }
            None => self.set_message("Nothing is selected"),
        }
    }

    fn yank_file(&mut self) {
        match self.get_selected() {
            Some(selected) => {
                let selected = &selected.path;
                if selected.is_file() {
                    self.register = selected.to_path_buf();
                    self.set_message("file in register, type p to paste");
                } else {
                    self.set_message("not a file")
                }
            }
            None => self.set_message("Nothing is selected"),
        }
    }

    fn paste_moved_file(&mut self) {
        let src = &self.register;
        let dst = PathBuf::new()
            .join(&self.pwd)
            .join(src.file_name().unwrap());
        match copy(&src, &dst) {
            Ok(_) => {
                match remove_file(&src) {
                    Ok(_) => {
                        self.refresh_middle_column();
                        let index = get_item_index(&dst, &self.middle_column.items);
                        self.set_message("deleted src, file moved!");
                        // select the moved file
                        self.middle_column.state.select(index)
                    }
                    Err(_) => {
                        self.refresh_middle_column();
                        self.set_message("copied file, but couldnt remove src");
                    }
                };
            }
            // might wanna verbalise those
            Err(_) => self.set_message("something went wrong while moving"),
        };
        self.register = PathBuf::new();
    }

    fn paste_yanked_file(&mut self) {
        let src = &self.register;
        let dst = PathBuf::new()
            .join(&self.pwd)
            .join(src.file_name().unwrap());
        match copy(src, &dst) {
            Ok(_) => {
                self.refresh_middle_column();
                let index = get_item_index(&dst, &self.middle_column.items);
                self.set_message("pasted!");
                // select the pasted file
                self.middle_column.state.select(index)
            }
            // might wanna verbalise those
            Err(_) => self.set_message("something went wrong while pasting"),
        };
        self.register = PathBuf::new();
    }

    // careful this only sorts the cwd for now, and forgets about it once its
    // gone out of view
    fn sort_by(&mut self, by: ListOrder) {
        self.orderby = by;
        self.refresh_all();
    }

    fn toggle_tag_item(&mut self) {
        match self.get_mut_selected() {
            Some(selected) => {
                selected.toggle_tagged();
                let selected = selected.path.to_path_buf();
                if !self.config.tags.contains(&selected) {
                    self.config.tags.push(selected);
                } else {
                    match self.config.tags.iter().position(|p| *p == selected) {
                        Some(pos) => {
                            self.config.tags.remove(pos);
                        }
                        None => self.set_message("selected item not in the list of tagged items"),
                    };
                };
            }
            None => self.set_message("nothing selected"),
        }
    }

    fn inc_search(&mut self) {
        let pattern = self.input_mode.get_str();
        if !pattern.starts_with('/') {
            // ayo wtf are you thinking calling this function without the proper
            // thing
            return;
        }
        let pattern = &pattern[1..].to_lowercase();
        let index = self.middle_column.items.iter().position(|item| {
            item.path
                .file_name()
                .unwrap()
                .to_str()
                .unwrap()
                .to_lowercase()
                .starts_with(pattern)
        });
        // when canceled it doesnt select anything so...
        self.middle_column.state.select(index);
        self.refresh_middle_column()
    }

    fn inc_find(&mut self) -> Option<usize> {
        let pattern = self.input_mode.get_str();
        if !pattern.starts_with(":find ") {
            // ayo wtf are you thinking calling this function without the proper
            // thing
            return None;
        }
        let pattern = &pattern[":find ".len()..].to_lowercase();
        // this mess here still needs A LOT of testing, it does not seem all that
        // proof against infinite loops...
        let index = self
            .middle_column
            .items
            .iter()
            .enumerate()
            .map(|(i, item)| {
                let mut score: Vec<usize> = Vec::new();
                pattern.chars().for_each(|c| {
                    let mut max = None;
                    loop {
                        eprintln!("char c = {c}");
                        match item
                            .path
                            .file_name()
                            .unwrap()
                            .to_str()
                            .unwrap()
                            .to_lowercase()
                            .chars()
                            .enumerate()
                            .position(|(i, x)| x == c && (Some(i) > max))
                        {
                            Some(pos) => {
                                eprintln!("pos = {pos} score = {score:?} i = {i} max = {max:?}");
                                if pos >= *score.iter().max().unwrap_or(&0) && !score.contains(&pos)
                                {
                                    score.push(pos);
                                    break;
                                } else {
                                    max = Some(pos);
                                }
                            }
                            None => break,
                        }
                    }
                });
                (i, score)
            })
            .inspect(|x| {
                eprintln!(
                    "about to filter this thing to get biggest elements {:?}, while length = {}",
                    x,
                    pattern.len()
                )
            })
            .filter(|(_, s)| s.iter().count() == pattern.len())
            .inspect(|x| eprintln!("getting the minimum of these {:?}", x))
            .min_by(|x, y| x.1.cmp(&y.1))
            .map(|x| x.0);

        self.middle_column.state.select(index);
        self.refresh_middle_column();
        index
    }
}

fn get_item_index<T>(item: &Path, items: &Vec<Item<PathBuf, T>>) -> Option<usize> {
    items.iter().position(|i| i.path.eq(item))
}

fn ls<T: std::cmp::Ord>(
    pwd: &Path,
    hidden: bool,
    order: &ListOrder,
    tags: &Vec<PathBuf>,
) -> Vec<Item<PathBuf, T>> {
    let paths = fs::read_dir(pwd);
    match paths {
        Ok(paths) => {
            let mut paths = paths
                .into_iter()
                .map(|p| p.unwrap().path())
                // filter hidden files or not depending on the hidden argument
                .filter(|p| !hidden || !p.file_name().unwrap().to_str().unwrap().starts_with("."))
                .map(|p| {
                    // ok now normally we should read this from the hashmap of
                    // tagged paths and see if the path is .. tagged.. lol
                    // TODO
                    let tagged = tags.contains(&p);
                    Item::new(p, tagged)
                })
                .collect::<Vec<Item<PathBuf, T>>>();

            let get_date_modified = |item: &Item<PathBuf, _>| match item.path.metadata() {
                Ok(metadata) => match metadata.modified() {
                    Ok(modified) => modified,
                    Err(_) => SystemTime::UNIX_EPOCH,
                },
                Err(_) => SystemTime::UNIX_EPOCH,
            };

            match order {
                ListOrder::Default => paths,
                ListOrder::Name => {
                    paths.sort();
                    paths
                }
                ListOrder::NameReverse => {
                    paths.sort();
                    paths.reverse();
                    paths
                }
                ListOrder::Modified => {
                    paths.sort_by(|a, b| get_date_modified(a).cmp(&get_date_modified(b)));
                    paths
                }
                ListOrder::ModifiedReverse => {
                    paths.sort_by(|a, b| get_date_modified(a).cmp(&get_date_modified(b)));
                    paths.reverse();
                    paths
                }
                ListOrder::DirsFirst => {
                    paths.sort_by(|a, b| a.path.is_file().cmp(&b.path.is_file()));
                    paths
                }
                ListOrder::FilesFirst => {
                    paths.sort_by(|a, b| a.path.is_dir().cmp(&b.path.is_dir()));
                    paths
                }
                _ => paths,
            }
        }
        Err(_) => {
            vec![]
        }
    }
}

fn main() -> Result<(), io::Error> {
    // setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    let args: Vec<String> = env::args().collect();
    let pwd: PathBuf;
    if args.len() == 1 {
        pwd = env::current_dir().unwrap();
    } else {
        let path = Path::new(args.get(1).unwrap());
        let exists = Path::exists(path);
        if exists {
            pwd = path.to_path_buf();
        } else {
            pwd = env::current_dir().unwrap();
        }
    }

    // create app and run it
    // take argument or get cwd
    let mut app = App::new(pwd, true);
    app.middle_column.state.select(Some(0));
    let res = run_app(&mut terminal, &mut app);
    confy::store("lga", Some("tags"), app.config).unwrap();

    // restore terminal
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    if let Err(err) = res {
        println!("{:?}", err)
    }

    Ok(())
}

fn run_app<B: Backend>(terminal: &mut Terminal<B>, app: &mut App) -> io::Result<()> {
    loop {
        terminal.draw(|f| ui::ui(f, app))?;

        if let Event::Key(key) = event::read()? {
            match app.input_mode {
                InputMode::Normal => match key.code {
                    KeyCode::Char('q') => return Ok(()),
                    KeyCode::Char('l') | KeyCode::Right => {
                        // go right
                        app.go_right();
                        app.set_metadata();
                    }
                    KeyCode::Char('k') | KeyCode::Up => {
                        // go up
                        app.middle_column.prev();
                        app.refresh_right_column();
                        app.set_metadata();
                        app.set_message("");
                    }
                    KeyCode::Char('j') | KeyCode::Down => {
                        // go down
                        app.middle_column.next();
                        app.refresh_right_column();
                        app.set_metadata();
                        app.set_message("");
                    }
                    KeyCode::Char('h') | KeyCode::Left => {
                        // go left
                        app.go_left();
                        app.set_metadata();
                        app.set_message("");
                    }
                    KeyCode::Char('g') | KeyCode::PageUp => {
                        // go to the beginning
                        app.middle_column
                            .state
                            .select(app.middle_column.items.len().gt(&0).then_some(0));
                        app.refresh_middle_column();
                        app.refresh_right_column();
                        app.set_metadata();
                        app.set_message("");
                    }
                    KeyCode::Char('G') | KeyCode::PageDown => {
                        // go to the end
                        app.middle_column
                            .state
                            .select(app.middle_column.items.len().checked_sub(1));
                        app.refresh_middle_column();
                        app.refresh_right_column();
                        app.set_metadata();
                        app.set_message("");
                    }
                    KeyCode::Char('d') => {
                        // implement deleting stuff
                        app.set_message("type D to delete or d to move");
                        app.input_mode = InputMode::Command("d".to_string());
                    }
                    KeyCode::Char('y') | KeyCode::Char('Y') => {
                        // yank stuff
                        app.set_message("type y to yank");
                        app.input_mode = InputMode::Command("y".to_string());
                    }
                    KeyCode::Char('s') => {
                        // sort
                        app.set_message(
                            "sort by name [n], modified date [m], dirs first [d], files first [f]",
                        );
                        app.input_mode = InputMode::Command("s".to_string());
                    }
                    KeyCode::Char('a') => match app.get_selected() {
                        Some(selected) => {
                            let selected = selected.path.file_name().unwrap().to_str().unwrap();
                            app.input_mode = InputMode::Input(format!(":rename {selected}"));
                            app.set_message(app.input_mode.get_str());
                        }
                        None => {
                            app.set_message("nothing is selected");
                        }
                    },
                    KeyCode::Char(':') => {
                        app.input_mode = InputMode::Input(":".to_string());
                        app.set_message(app.input_mode.get_str());
                    }
                    KeyCode::Backspace => {
                        app.toggle_hidden_files();
                        app.refresh_all();
                        app.set_metadata();
                    }
                    KeyCode::Char('t') => {
                        app.toggle_tag_item();
                    }
                    KeyCode::Char('/') => {
                        // implement incremental search
                        app.input_mode = InputMode::Input("/".to_string());
                        app.set_message(app.input_mode.get_str());
                    }
                    KeyCode::Char('f') => {
                        // implement incremental search
                        app.input_mode = InputMode::Input(":find ".to_string());
                        app.set_message(app.input_mode.get_str());
                    }
                    KeyCode::Char(' ') => {
                        // select the current thing
                        // so options huh... we have a vec in the global app
                        // state that contains the selected paths...
                        // but this vec has to be only ... i think we should
                        // implement tagging first, itll make this easier to
                        // reason about i guess
                    }
                    _ => {}
                },
                InputMode::Command(ref mut command) => match key.code {
                    KeyCode::Char('q') => return Ok(()),
                    KeyCode::Char('l') | KeyCode::Right => {
                        // go right
                        app.go_right();
                        app.set_metadata();
                    }
                    KeyCode::Char('k') | KeyCode::Up => {
                        // go up
                        app.middle_column.prev();
                        app.refresh_right_column();
                        app.set_metadata();
                        app.set_message("");
                    }
                    KeyCode::Char('j') | KeyCode::Down => {
                        // go down
                        app.middle_column.next();
                        app.refresh_right_column();
                        app.set_metadata();
                        app.set_message("");
                    }
                    KeyCode::Char('h') | KeyCode::Left => {
                        // go left
                        app.go_left();
                        app.set_metadata();
                        app.set_message("");
                    }
                    KeyCode::Char('g') | KeyCode::PageUp => {
                        // go to the beginning
                        app.middle_column
                            .state
                            .select(app.middle_column.items.len().gt(&0).then_some(0));
                        app.refresh_middle_column();
                        app.refresh_right_column();
                        app.set_metadata();
                        app.set_message("");
                    }
                    KeyCode::Char('G') | KeyCode::PageDown => {
                        // go to the end
                        app.middle_column
                            .state
                            .select(app.middle_column.items.len().checked_sub(1));
                        app.refresh_middle_column();
                        app.refresh_right_column();
                        app.set_metadata();
                        app.set_message("");
                    }
                    KeyCode::Backspace => {
                        app.toggle_hidden_files();
                        app.refresh_all();
                        app.set_metadata();
                    }
                    KeyCode::Char(c) => {
                        command.push(c);
                        match command.as_str() {
                            "dD" => {
                                app.input_mode = InputMode::Normal;
                                app.delete_file();
                            }
                            "dd" | "yy" => app.yank_file(),
                            "ddp" => {
                                app.input_mode = InputMode::Normal;
                                app.paste_moved_file();
                            }
                            "yyp" => {
                                app.input_mode = InputMode::Normal;
                                app.paste_yanked_file();
                            }
                            "sn" => {
                                // sort by name
                                app.input_mode = InputMode::Normal;
                                app.sort_by(ListOrder::Name);
                            }
                            "sN" => {
                                // sort by name in reverse
                                app.input_mode = InputMode::Normal;
                                app.sort_by(ListOrder::NameReverse);
                            }
                            "sm" => {
                                // sort by modified
                                app.input_mode = InputMode::Normal;
                                app.sort_by(ListOrder::Modified);
                            }
                            "sM" => {
                                // sort by modified
                                app.input_mode = InputMode::Normal;
                                app.sort_by(ListOrder::ModifiedReverse);
                            }
                            "sd" => {
                                // sort by type: dirs first
                                app.input_mode = InputMode::Normal;
                                app.sort_by(ListOrder::DirsFirst);
                            }
                            "sf" => {
                                // sort by type: files first
                                app.input_mode = InputMode::Normal;
                                app.sort_by(ListOrder::FilesFirst);
                            }
                            _ => {
                                app.input_mode = InputMode::Normal;
                                app.set_message("command not found");
                            }
                        }
                    }
                    KeyCode::Esc => {
                        app.input_mode = InputMode::Normal;
                        app.set_message("canceled");
                    }
                    _ => {}
                },
                InputMode::Input(_) => match key.code {
                    KeyCode::Char(c) => {
                        app.input_mode.push(c);
                        app.set_message(app.input_mode.get_str());
                        if app.input_mode.get_str().starts_with('/') {
                            // incrementally highlight the found thing
                            app.inc_search();
                        } else if app.input_mode.get_str().starts_with(":find ") {
                            // incrementally highlight the found thing
                            app.inc_find();
                        }
                    }
                    KeyCode::Enter => {
                        // execute the command somehow
                        app.execute();
                        app.input_mode = InputMode::Normal;
                    }
                    KeyCode::Backspace => {
                        app.input_mode.pop();
                        app.set_message(app.input_mode.get_str());
                        // a bit of a special case here for find
                        if app.input_mode.get_str().starts_with(":find") {
                            app.inc_find();
                        }
                    }
                    KeyCode::Esc => {
                        app.set_message("canceled");
                        app.input_mode = InputMode::Normal;
                    }
                    _ => {}
                },
                InputMode::Confirmation(_) => match key.code {
                    KeyCode::Char('y') => {
                        app.confirm();
                        app.input_mode = InputMode::Normal;
                    }
                    _ => {
                        app.set_message("aborted");
                        app.input_mode = InputMode::Normal;
                    }
                },
            }
        }
    }
}
