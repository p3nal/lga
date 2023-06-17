mod ui;
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use file_format::{FileFormat, Kind};
use humansize::{format_size, DECIMAL};
use std::{
    env,
    fs::{
        self, copy, create_dir, remove_dir, remove_dir_all, remove_file, rename, File, Metadata,
        ReadDir,
    },
    io,
    os::unix::prelude::MetadataExt,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, Instant, SystemTime},
};
use tui::{
    backend::{Backend, CrosstermBackend},
    widgets::ListState,
    Terminal,
};

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
    // visual mode for selecting stuff
    Visual(Vec<PathBuf>),
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

pub struct App {
    left_column: Vec<PathBuf>,
    middle_column: StatefulList<PathBuf>,
    right_column: Vec<PathBuf>,
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
    tag_register: Vec<PathBuf>,
}

impl App {
    fn new(pwd: PathBuf, hidden: bool) -> App {
        // list the parent stuff
        let left_column = match pwd.parent() {
            Some(parent) => ls(parent, hidden, &ListOrder::DirsFirst),
            None => vec![],
        };
        // list pwd stuff
        let middle_column = ls(&pwd, hidden, &ListOrder::DirsFirst);
        // list child stuff
        let right_column = ls(
            &middle_column
                .get(0)
                .unwrap_or(&PathBuf::default())
                .as_path(),
            hidden,
            &ListOrder::DirsFirst,
        );
        App {
            left_column,
            middle_column: StatefulList {
                items: middle_column,
                state: ListState::default(),
            },
            right_column,
            orderby: ListOrder::Default,
            pwd: pwd.to_path_buf(),
            hidden,
            message: String::new(),
            metadata: String::new(),
            input_mode: InputMode::Normal,
            register: PathBuf::new(),
            tag_register: vec![],
        }
    }

    fn go_right(&mut self) {
        match self.get_selected() {
            Some(selected) => {
                if selected.is_dir() {
                    // empty status, we gon need
                    // this just seems like a lot of work
                    // let empty = match selected.read_dir() {
                    //     Ok(mut readdir) => readdir.next().is_none(),
                    //     Err(_) => false,
                    // };
                    self.pwd = selected.to_path_buf();
                    self.left_column = self.middle_column.items.to_owned();
                    self.middle_column.items = self.right_column.to_owned();
                    // maybe remove this? and deal with the errors lol
                    // i think its best if we check if theres any selected and
                    // then if none is, do select...
                    // fuck it. i think what i should be doing is copy the state
                    // to each one of the three things.. damn that would suck
                    // cuz then again i would need to do it beyond.. no way
                    let empty = self.middle_column.items.len() < 1;
                    if !empty {
                        self.middle_column.state.select(Some(0));
                    } else {
                        self.middle_column.state.select(None);
                    }
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
                            Command::new("/usr/bin/nvim")
                                .arg(selected.as_path())
                                .stderr(Stdio::null())
                                .spawn()
                                .expect("failed to exec");
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
            None => {}
        }
    }

    fn go_left(&mut self) {
        // we have to somehow select the parent when going left
        match self.pwd.parent() {
            Some(parent) => {
                let parent_index: Option<usize> = get_item_index(&self.pwd, &self.left_column);
                self.right_column = self.middle_column.items.to_owned();
                self.middle_column.items = self.left_column.to_owned();
                self.middle_column.state.select(parent_index);
                self.pwd = parent.to_path_buf();
                match self.pwd.parent() {
                    Some(parent) => self.left_column = self.ls(parent),
                    None => self.left_column = vec![],
                }
            }
            None => {}
        };
    }

    fn refresh_right_column(&mut self) {
        match self.get_selected() {
            Some(selected) => {
                let path = selected.as_path();
                if selected.is_dir() {
                    self.right_column = self.ls(&path)
                } else if selected.is_file() {
                    self.right_column = vec![];
                } else {
                    // just cuz it probably needs to be handled later
                    self.right_column = vec![];
                }
            }
            None => {}
        }
    }

    fn refresh_left_column(&mut self) {
        match self.pwd.parent() {
            Some(parent) => self.left_column = self.ls(parent),
            None => self.left_column = vec![],
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

    fn get_selected(&self) -> Option<&PathBuf> {
        self.middle_column
            .items
            .get(self.middle_column.state.selected().unwrap_or(0))
    }

    fn ls(&self, pwd: &Path) -> Vec<PathBuf> {
        ls(pwd, self.hidden, &self.orderby)
    }

    fn set_metadata(&mut self) {
        let size = match self.get_selected() {
            Some(selected) => match selected.metadata() {
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
        let command = command.split_once(' ').unwrap_or(("", ""));
        match command.0 {
            ":rename" => {
                let src = self.get_selected().unwrap();
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
            _ => {
                // make this into some easter egg, randomize statements and throw
                // them in for a pinch of fun
                self.set_message("i traveled the earth to find your command and couldnt find it")
            }
        }
    }

    fn confirm(&mut self) {
        match self.input_mode {
            InputMode::Confirmation(Confirm::DeleteFolder) => match self.get_selected() {
                Some(selected) => {
                    // delete all
                    match remove_dir_all(selected.as_path()) {
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

    fn tag_item(&mut self) {
        match self.get_selected() {
            Some(selected) => {
                let selected = selected.to_path_buf();
                if !self.tag_register.contains(&selected) {
                    self.tag_register.push(selected);
                };
            },
            None => self.set_message("nothing selected"),
        }
    }
}

fn get_item_index(item: &Path, items: &Vec<PathBuf>) -> Option<usize> {
    items.into_iter().position(|i| i.eq(item))
}

fn ls(pwd: &Path, hidden: bool, order: &ListOrder) -> Vec<PathBuf> {
    let paths = fs::read_dir(pwd);
    match paths {
        Ok(paths) => {
            let mut paths = paths
                .into_iter()
                .map(|p| p.unwrap().path())
                // filter hidden files or not depending on the hidden argument
                .filter(|p| !hidden || !p.file_name().unwrap().to_str().unwrap().starts_with("."))
                .collect::<Vec<PathBuf>>();

            let get_date_modified = |p: &PathBuf| match p.metadata() {
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
                    paths.sort_by(|a, b| a.is_file().cmp(&b.is_file()));
                    paths
                }
                ListOrder::FilesFirst => {
                    paths.sort_by(|a, b| a.is_dir().cmp(&b.is_dir()));
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
    let tick_rate = Duration::from_millis(250);
    // take argument or get cwd
    let mut app = App::new(pwd, true);
    app.middle_column.state.select(Some(0));
    let res = run_app(&mut terminal, app, tick_rate);

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

fn run_app<B: Backend>(
    terminal: &mut Terminal<B>,
    mut app: App,
    tick_rate: Duration,
) -> io::Result<()> {
    let last_tick = Instant::now();
    loop {
        terminal.draw(|f| ui::ui(f, &mut app))?;

        let timeout = tick_rate
            .checked_sub(last_tick.elapsed())
            .unwrap_or_else(|| Duration::from_secs(0));
        if crossterm::event::poll(timeout)? {
            if let Event::Key(key) = event::read()? {
                match app.input_mode {
                    InputMode::Normal => match key.code {
                        KeyCode::Char('q') => return Ok(()),
                        KeyCode::Char('l') => {
                            // go right
                            app.go_right();
                            app.set_metadata();
                        }
                        KeyCode::Char('k') => {
                            // go up
                            app.middle_column.prev();
                            app.refresh_right_column();
                            app.set_metadata();
                            app.set_message("");
                        }
                        KeyCode::Char('j') => {
                            // go down
                            app.middle_column.next();
                            app.refresh_right_column();
                            app.set_metadata();
                            app.set_message("");
                        }
                        KeyCode::Char('h') => {
                            // go left
                            app.go_left();
                            app.set_metadata();
                            app.set_message("");
                        }
                        KeyCode::Char('g') => {
                            // go to the beginning
                            app.middle_column
                                .state
                                .select(app.middle_column.items.len().gt(&0).then_some(0));
                            app.refresh_middle_column();
                            app.refresh_right_column();
                            app.set_metadata();
                            app.set_message("");
                        }
                        KeyCode::Char('G') => {
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
                                let selected = selected.file_name().unwrap().to_str().unwrap();
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
                            app.tag_item();
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
                        KeyCode::Char('l') => {
                            // go right
                            app.go_right();
                            app.set_metadata();
                        }
                        KeyCode::Char('k') => {
                            // go up
                            app.middle_column.prev();
                            app.refresh_right_column();
                            app.set_metadata();
                            app.set_message("");
                        }
                        KeyCode::Char('j') => {
                            // go down
                            app.middle_column.next();
                            app.refresh_right_column();
                            app.set_metadata();
                            app.set_message("");
                        }
                        KeyCode::Char('h') => {
                            // go left
                            app.go_left();
                            app.set_metadata();
                            app.set_message("");
                        }
                        KeyCode::Char('g') => {
                            // go to the beginning
                            app.middle_column.state.select(Some(0));
                            app.refresh_middle_column();
                            app.refresh_right_column();
                            app.set_metadata();
                            app.set_message("");
                        }
                        KeyCode::Char('G') => {
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
                            app.set_message(app.input_mode.get_str())
                        }
                        KeyCode::Enter => {
                            // execute the command somehow
                            app.execute();
                            app.input_mode = InputMode::Normal;
                        }
                        KeyCode::Backspace => {
                            app.input_mode.pop();
                            app.set_message(app.input_mode.get_str())
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
                    InputMode::Visual(ref selected) => {}
                }
            }
        }
    }
}
