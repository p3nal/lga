use crate::App;
use std::ffi::OsStr;
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
                Constraint::Percentage(30),
                Constraint::Percentage(40),
                Constraint::Percentage(30),
            ]
            .as_ref(),
        )
        .split(vertical_chunks[1]);
    // Create a block...
    let left_column_list: Vec<ListItem> = app
        .left_column
        .iter()
        .map(|item| ListItem::new(item.file_name().unwrap().to_str().unwrap()))
        .collect();

    let middle_column_list: Vec<ListItem> = app
        .middle_column
        .items
        .iter()
        .map(|item| {
            if item.is_dir() {
                ListItem::new(item.file_name().unwrap().to_str().unwrap())
                    .style(Style::default().fg(Color::LightGreen))
            } else {
                ListItem::new(item.file_name().unwrap().to_str().unwrap())
                    .style(Style::default().fg(Color::Gray))
            }
        })
        .collect();

    let right_column_list: Vec<ListItem> = app
        .right_column
        .iter()
        .map(|item| ListItem::new(item.file_name().unwrap().to_str().unwrap()))
        .collect();

    let left_block = List::new(left_column_list)
        .block(Block::default()./*title("Parent").*/borders(Borders::ALL).border_type(BorderType::Rounded))
        .style(Style::default().fg(Color::Blue))
        .highlight_style(Style::default().add_modifier(Modifier::ITALIC))
        .highlight_symbol(">>");

    let middle_block = List::new(middle_column_list)
        .block(
            Block::default()
                // .title(
                //     app.pwd
                //         .file_name()
                //         .unwrap_or(OsStr::new("/"))
                //         .to_str()
                //         .unwrap(),
                // )
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded),
        )
        .style(Style::default().fg(Color::White))
        .highlight_style(
            Style::default()
                .bg(Color::LightGreen)
                .fg(Color::Black)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol(" ");

    let right_block = List::new(right_column_list)
        .block(
            Block::default() /* .title("Child") */
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded),
        )
        .style(Style::default().fg(Color::Red))
        .highlight_style(Style::default().add_modifier(Modifier::ITALIC))
        .highlight_symbol(">>");

    // header
    let header = app
        .middle_column
        .items
        .get(app.middle_column.state.selected().unwrap_or(0))
        .unwrap()
        .display()
        .to_string();
    let header = Paragraph::new(header)
        .style(Style::default().fg(Color::Magenta))
        .alignment(Alignment::Left);

    // footer(s)
    let metadata = Paragraph::new(app.metadata.as_ref()).alignment(Alignment::Right);
    // .block(Block::default().borders(Borders::RIGHT));

    let message = Paragraph::new(app.message.as_ref()).alignment(Alignment::Left);
    // .block(Block::default().borders(Borders::LEFT));

    // Render into chunks of the layout.
    frame.render_widget(header, vertical_chunks[0]);
    frame.render_widget(left_block, chunks[0]);
    frame.render_stateful_widget(middle_block, chunks[1], &mut app.middle_column.state);
    frame.render_widget(right_block, chunks[2]);
    frame.render_widget(metadata, vertical_chunks[2]);
    frame.render_widget(message, vertical_chunks[2]);
}
