mod ui;
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use file_format::{FileFormat, Kind};
use std::{
    env::{self, join_paths},
    fs::{self, copy, remove_dir, remove_dir_all, remove_file, rename},
    io,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, Instant},
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

    fn prev(&mut self) {
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

enum InputMode {
    Normal,
    Editing,
}

pub struct App {
    left_column: Vec<PathBuf>,
    middle_column: StatefulList<PathBuf>,
    right_column: Vec<PathBuf>,
    pwd: PathBuf,
    hidden: bool,
    message: String,
    metadata: String,
    /// Current value of the input command
    input: String,
    /// Current input mode
    input_mode: InputMode,
    // register for yanking and moving
    register: PathBuf,
}

impl App {
    fn new(pwd: PathBuf, hidden: bool) -> App {
        // list the parent stuff
        let left_column = match pwd.parent() {
            Some(parent) => ls(parent, hidden),
            None => {
                vec![]
            }
        };
        // list pwd stuff
        let middle_column = ls(&pwd, hidden);
        // list child stuff
        let right_column = ls(&middle_column.get(0).unwrap().as_path(), hidden);
        App {
            left_column,
            middle_column: StatefulList {
                items: middle_column,
                state: ListState::default(),
            },
            right_column,
            pwd: pwd.to_path_buf(),
            hidden,
            message: String::new(),
            metadata: String::new(),
            input_mode: InputMode::Normal,
            input: String::new(),
            register: PathBuf::new(),
        }
    }

    fn go_right(&mut self) {
        match self.get_selected() {
            Some(selected) => {
                if selected.is_dir() {
                    // check if directory is empty before proceeding
                    if selected.read_dir().unwrap().next().is_none() {
                        self.set_message("directory empty");
                        return;
                    }
                    self.pwd = selected.to_path_buf();
                    self.left_column = self.middle_column.items.to_owned();
                    self.middle_column.items = self.right_column.to_owned();
                    // maybe remove this? and deal with the errors lol
                    // i think its best if we check if theres any selected and
                    // then if none is, do select...
                    // fuck it. i think what i should be doing is copy the state
                    // to each one of the three things.. damn that would suck
                    // cuz then again i would need to do it beyond.. no way
                    self.middle_column.state.select(Some(0));
                    // check if dir is empty first...
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
                        Kind::Text => {
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
                let parent_index: usize = get_item_index(&self.pwd, &self.left_column);
                self.right_column = self.middle_column.items.to_owned();
                self.middle_column.items = self.left_column.to_owned();
                self.middle_column.state.select(Some(parent_index));
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

    fn ls(&self, pwd: &Path) -> Vec<PathBuf> {
        ls(pwd, self.hidden)
    }

    fn set_metadata(&mut self) {
        let count = &self.middle_column.items.len();
        let index = &self.middle_column.state.selected().unwrap_or(0) + 1;
        self.metadata = format!("{index}/{count} ")
    }

    fn set_message<T: AsRef<str>>(&mut self, message: T) {
        self.message = message.as_ref().to_string()
    }

    // a good thing to do is to make a trash folder and collect stuff to delete
    // there, then just before the app closes we can issue a delete...
    // although this may make exiting a bit slower i recon... anyway
    fn delete_file(&mut self) {
        match self.get_selected() {
            Some(selected) => {
                let path = selected.as_path();
                if selected.is_dir() {
                    // this should require confirmation or something like damn bro
                    // remove_dir_all(path)
                    match remove_dir(path) {
                        Ok(_) => self.set_message("deleted"),
                        Err(_) => self.set_message("wont delete"),
                    };
                } else if selected.is_file() {
                    // this should require confirmation or something like damn bro
                    match remove_file(path) {
                        Ok(_) => self.set_message("deleted"),
                        Err(_) => self.set_message("wont delete"),
                    };
                    // self.set_message("are you sure you want to do this");
                }
                self.refresh_middle_column();
            }
            None => {}
        }
    }

    fn execute(&mut self) {
        let command = self.input.drain(..).collect::<String>();
        match command.as_str() {
            _ => {}
        }
    }

    // TODO gotta use this more
    fn get_selected(&self) -> Option<&PathBuf> {
        self.middle_column
            .items
            .get(self.middle_column.state.selected().unwrap_or(0))
    }

    fn yank_file(&mut self) {
        let selected = self.get_selected().unwrap();
        if selected.is_file() {
            self.register = selected.to_path_buf();
            self.set_message("file in register, type p to paste");
        } else {
            self.set_message("not a file")
        }
    }

    fn paste_moved_file(&mut self) {
        let src = &self.register;
        let dst = PathBuf::new()
            .join(&self.pwd)
            .join(src.file_name().unwrap());
        match copy(src, dst) {
            Ok(_) => {
                match remove_file(src) {
                    Ok(_) => self.set_message("deleted src, file moved!"),
                    Err(_) => {
                        self.set_message("something went wrong while moving")
                    }
                };
                // self.set_message("moved!")
            }
            // might wanna verbalise those
            Err(_) => {
                self.set_message("something went wrong while moving")
            }
        };
        self.register = PathBuf::new();
        self.refresh_middle_column();
    }

    fn paste_yanked_file(&mut self) {
        let src = &self.register;
        let dst = PathBuf::new()
            .join(&self.pwd)
            .join(src.file_name().unwrap());
        match copy(src, dst) {
            Ok(_) => self.set_message("pasted!"),
            // might wanna verbalise those
            Err(_) => self.set_message("something went wrong while pasting"),
        };
        self.register = PathBuf::new();
        self.refresh_middle_column();
    }
}

fn get_item_index(parent: &Path, items: &Vec<PathBuf>) -> usize {
    items.into_iter().position(|p| p.eq(parent)).unwrap_or(0)
}

fn ls(pwd: &Path, hidden: bool) -> Vec<PathBuf> {
    let paths = fs::read_dir(pwd);
    // you might wanna think about making return a Result<Vec<PathBuf>, Error>
    match paths {
        Ok(paths) => paths
            .into_iter()
            .map(|p| p.unwrap().path())
            // filter hidden files or not depending on the hidden argument
            .filter(|p| !hidden || !p.file_name().unwrap().to_str().unwrap().starts_with("."))
            .collect(),
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
                                .select(Some(app.middle_column.items.len() - 1));
                            app.refresh_middle_column();
                            app.refresh_right_column();
                            app.set_metadata();
                            app.set_message("");
                        }
                        KeyCode::Char('d') => {
                            // implement deleting stuff
                            app.input_mode = InputMode::Editing;
                            app.input.push('d');
                        }
                        KeyCode::Char('y') => {
                            // yank stuff
                            app.input_mode = InputMode::Editing;
                            app.input.push('y');
                        }
                        KeyCode::Backspace => {
                            app.toggle_hidden_files();
                            app.refresh_all();
                            app.set_metadata();
                        }
                        _ => {}
                    },
                    InputMode::Editing => match key.code {
                        KeyCode::Enter => {
                            // execute the command somehow
                            app.input_mode = InputMode::Normal;
                            app.execute();
                        }
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
                                .select(Some(app.middle_column.items.len() - 1));
                            app.refresh_middle_column();
                            app.refresh_right_column();
                            app.set_metadata();
                            app.set_message("");
                        }
                        KeyCode::Char(c) => {
                            app.input.push(c);
                            match c {
                                'D' => {
                                    if app.input.drain(..).collect::<String>().eq("dD") {
                                        app.input_mode = InputMode::Normal;
                                        app.delete_file()
                                    }
                                }
                                'd' => {
                                    if app.input.eq("dd") {
                                        // app.input_mode = InputMode::Normal;
                                        app.yank_file()
                                    }
                                }
                                'y' => {
                                    if app.input.eq("yy") {
                                        // app.input_mode = InputMode::Normal;
                                        app.yank_file()
                                    }
                                }
                                'p' => {
                                    let input = app.input.drain(..).collect::<String>();
                                    // are these checks necessary?
                                    if input.eq("yyp") {
                                        app.input_mode = InputMode::Normal;
                                        app.paste_yanked_file();
                                    } else if input.eq("ddp") {
                                        app.input_mode = InputMode::Normal;
                                        app.paste_moved_file();
                                    }
                                }
                                _ => {}
                            }
                        }
                        KeyCode::Backspace => {
                            app.input.pop();
                        }

                        KeyCode::Esc => {
                            app.input_mode = InputMode::Normal;
                        }
                        _ => {}
                    },
                }
            }
        }
    }
}
