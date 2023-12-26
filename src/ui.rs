use crate::App;
use tui::{
    backend::Backend,
    layout::{Alignment, Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    widgets::{Block, BorderType, Borders, List, ListItem, Paragraph},
    Frame,
};

pub fn ui<B: Backend>(frame: &mut Frame<B>, app: &mut App) {
    let vertical_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints(
            [
                Constraint::Percentage(3),
                Constraint::Percentage(96),
                Constraint::Percentage(1),
            ]
            .as_ref(),
        )
        .split(frame.size());
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints(
            [
                Constraint::Percentage(20),
                Constraint::Percentage(50),
                Constraint::Percentage(30),
            ]
            .as_ref(),
        )
        .split(vertical_chunks[1]);
    // Create a block...
    let left_column_list: Vec<ListItem> = app
        .left_column
        .items
        .iter()
        .map(|item| ListItem::new(item.path.file_name().unwrap().to_str().unwrap()))
        .collect();

    let middle_column_list: Vec<ListItem> = app
        .middle_column
        .items
        .iter()
        .map(|item| {
            let tagged = if item.tagged { '*' } else { ' ' };
            let item = &item.path;
            let selected = 
                    match &app.input_mode {
                        crate::InputMode::Select(v) => {
                            if v.contains(item) {
                                " "
                            } else {
                                ""
                            }
                        }
                        _ => "",
                    };
            // deal with those unwraps man
            if item.is_dir() {
                ListItem::new(format!(
                    "{tagged}{selected}{}",
                    item.file_name().unwrap().to_str().unwrap(),
                ))
                .style(Style::default().fg(Color::Green))
            } else {
                ListItem::new(format!(
                    "{tagged}{selected}{}",
                    item.file_name().unwrap().to_str().unwrap()
                ))
                .style(Style::default().fg(Color::Gray))
            }
        })
        .collect();

    let right_column_list: Vec<ListItem> = app
        .right_column
        .items
        .iter()
        .map(|item| ListItem::new(item.path.file_name().unwrap().to_str().unwrap()))
        .collect();

    let left_block = List::new(left_column_list)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded),
        )
        .style(Style::default().fg(Color::LightBlue).add_modifier(Modifier::BOLD));

    let middle_block = List::new(middle_column_list)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded),
        )
        .style(Style::default().fg(Color::White).add_modifier(Modifier::BOLD))
        .highlight_style(
            Style::default()
                .bg(Color::Green)
                .fg(Color::Black)
                .add_modifier(Modifier::BOLD),
        );

    let right_block = List::new(right_column_list)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded),
        )
        .style(Style::default().fg(Color::LightRed).add_modifier(Modifier::BOLD));

    // header
    let header = match app.get_selected() {
        Some(selected) => selected.path.display().to_string(),
        None => app.pwd.display().to_string(),
    };
    let header = Paragraph::new(header)
        .style(Style::default().fg(Color::Magenta))
        .alignment(Alignment::Left);

    // footer(s)
    let metadata = Paragraph::new(app.metadata.as_ref()).alignment(Alignment::Right);
    let message = Paragraph::new(app.message.as_ref()).alignment(Alignment::Left);

    // Render into chunks of the layout.
    frame.render_widget(header, vertical_chunks[0]);
    frame.render_widget(left_block, chunks[0]);
    frame.render_stateful_widget(middle_block, chunks[1], &mut app.middle_column.state);
    frame.render_widget(right_block, chunks[2]);
    frame.render_widget(metadata, vertical_chunks[2]);
    frame.render_widget(message, vertical_chunks[2]);
}
