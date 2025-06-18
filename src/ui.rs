// ui.rs

use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout},
    style::{Color, Modifier, Style, Stylize},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
};

use crate::app::{App, AppMode, CurrentScreen, GeoJsonInfo};

pub fn render(frame: &mut Frame, app: &mut App) {
    let main_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(0), Constraint::Length(3)]) // Main content, then footer
        .split(frame.size());

    match app.current_screen {
        CurrentScreen::Main => render_main_screen(frame, app, main_layout[0]),
        CurrentScreen::Help => render_help_screen(frame, app, main_layout[0]),
        CurrentScreen::GeoJsonMapper => render_geojson_mapper_ui(frame, app, main_layout[0]), // GeoJSON Mapper is now the main screen
    }

    // Render the footer, common across all screens
    render_footer(frame, app, main_layout[1]);
}

/// Renders the main application screen
fn render_main_screen(frame: &mut Frame, _app: &mut App, area: ratatui::layout::Rect) {
    let block = Block::default()
        .title(" Rust GeoJson Mapper TUI")
        .title_style(Style::default().fg(Color::Cyan).bold())
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Magenta));

    let content = Paragraph::new(format!(
        "Welcome to the GeoJSON Mapper TUI!\n\n\
        This is the main application screen.\n\n\
        The GeoJSON Mapper UI is now the default view.\n\
        Press 'q' to quit (or return to this screen from Help).\n\
        Press 'h' for help."
    ))
    .block(block)
    .wrap(Wrap { trim: false })
    .style(Style::default().fg(Color::White));

    frame.render_widget(content, area);
}

/// Renders the help screen.
fn render_help_screen(frame: &mut Frame, _app: &mut App, area: ratatui::layout::Rect) {
    let block = Block::default()
        .title(" Help Screen ")
        .title_style(Style::default().fg(Color::Yellow).bold())
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Blue));

    let help_text = Paragraph::new(
        "Keybinds:\n\
          J/K or ↑/↓: Navigate file list\n\
          Space: Toggle file selection\n\
          Enter: Plot selected files\n\
          C: Cycle next assignment color\n\
          R: Rename output plot\n\
          /: Start fuzzy search\n\
          P: Toggle Points visibility\n\
          L: Toggle Lines visibility\n\
          O: Toggle Polygons visibility\n\
          Q: Quit the application\n\
          H: Show this Help screen\n\n\
          Click & Drag Divider: Resize panels in GeoJSON Mapper UI.",
    )
    .block(block)
    .wrap(Wrap { trim: false })
    .style(Style::default().fg(Color::LightGreen));

    frame.render_widget(help_text, area);
}

// Renders the GeoJSON Mapper UI
fn render_geojson_mapper_ui(frame: &mut Frame, app: &mut App, area: ratatui::layout::Rect) {
    // Main vertical layout: Notification/Search, then Main Content, then Spacer
    let main_layout_constraints = if app.current_mode == AppMode::Searching {
        vec![
            Constraint::Length(1), // Notification
            Constraint::Length(1), // Spacer
            Constraint::Length(1), // Search bar
            Constraint::Min(0),    // Main content area
            Constraint::Length(1), // Spacer
        ]
    } else {
        vec![
            Constraint::Length(1), // Notification
            Constraint::Length(1), // Spacer
            Constraint::Min(0),    // Main content area
            Constraint::Length(1), // Spacer
        ]
    };

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints(main_layout_constraints)
        .split(area);

    let mut current_chunk_idx = 0;

    // Notification Area
    let notification_paragraph = Paragraph::new(app.notification.clone())
        .alignment(Alignment::Center)
        .style(Style::default().fg(Color::White).bg(Color::DarkGray));
    frame.render_widget(notification_paragraph, chunks[current_chunk_idx]);
    current_chunk_idx += 1;

    // Spacer
    frame.render_widget(Paragraph::new(""), chunks[current_chunk_idx]);
    current_chunk_idx += 1;

    // Search Bar (conditional)
    if app.current_mode == AppMode::Searching {
        let search_layout = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Length(8), // "Search:" label
                Constraint::Min(0),    // Input field
            ])
            .split(chunks[current_chunk_idx]);

        let search_label = Paragraph::new("Search:").style(Style::default().fg(Color::LightCyan));
        frame.render_widget(search_label, search_layout[0]);

        let search_input_paragraph = Paragraph::new(app.search_query_buffer.clone())
            .style(Style::default().fg(Color::Yellow));

        frame.render_widget(search_input_paragraph, search_layout[1]);

        if app.current_mode == AppMode::Searching {
            frame.set_cursor(
                search_layout[1].x + app.search_query_cursor as u16,
                search_layout[1].y,
            );
        }
        current_chunk_idx += 1;
    }

    // Main Content Area (Left Panel + Right Panels)
    let main_content_area = chunks[current_chunk_idx];
    let main_content_layout = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(app.left_pane_width_percentage), // Dynamically sized left pane
            Constraint::Percentage(100 - app.left_pane_width_percentage), // Dynamically sized right pane
        ])
        .split(main_content_area);

    let left_panel_area = main_content_layout[0];
    let right_panel_area = main_content_layout[1];

    // --- Left Panel: GeoJSON File List ---
    let left_panel_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // Title
            Constraint::Min(0),    // File list
        ])
        .split(left_panel_area);

    let file_list_title = Paragraph::new(" Available GeoJSON files: ")
        .block(Block::default().borders(Borders::BOTTOM))
        .style(Style::default().fg(Color::LightGreen).bold());
    frame.render_widget(file_list_title, left_panel_chunks[0]);

    // File List Items
    let mut list_items: Vec<Line> = Vec::new();
    let max_visible_items_in_list = left_panel_chunks[1].height as usize;
    let end_display_index =
        (app.scroll_offset + max_visible_items_in_list).min(app.filtered_geojson_indices.len());

    for i in app.scroll_offset..end_display_index {
        let original_index = app.filtered_geojson_indices[i];
        let file_name = &app.geojson_files[original_index];
        let selection_indicator = if app.selected_files_status[original_index] {
            "[x]"
        } else {
            "[ ]"
        };
        let display_text = format!(
            "{} {}. {}",
            selection_indicator,
            original_index + 1,
            file_name
        );
        let mut style = Style::default().fg(Color::White);
        if i == app.selected_file_index {
            style = style.bg(Color::DarkGray).add_modifier(Modifier::BOLD);
        }
        if app.selected_files_status[original_index] {
            if let Some(color_rgb) = app.assigned_plot_colors[original_index] {
                style = style.fg(Color::Rgb(color_rgb.0, color_rgb.1, color_rgb.2));
            }
        }
        list_items.push(Line::from(vec![Span::styled(display_text, style)]));
    }

    let file_list_paragraph = Paragraph::new(list_items)
        .block(Block::default().borders(Borders::ALL).title("Files"))
        .wrap(Wrap { trim: false });
    frame.render_widget(file_list_paragraph, left_panel_chunks[1]);

    // --- Right Panel ---
    let right_panel_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage(35), // File Info
            Constraint::Length(6),      // Plotting Options
            Constraint::Min(0),         // Help/Keybinds
        ])
        .split(right_panel_area);

    // Section 1: Detailed File Information
    let file_info_block = Block::default()
        .title(" File Information ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::LightBlue));

    let current_original_file_index = if app.filtered_geojson_indices.is_empty() {
        0
    } else {
        app.filtered_geojson_indices[app
            .selected_file_index
            .min(app.filtered_geojson_indices.len().saturating_sub(1))]
    };

    let mut file_info_text = Vec::new();
    if let Some(info) = &app.cached_geojson_info[current_original_file_index] {
        file_info_text.push(Line::from(format!("Size: {} KB", info.file_size_kb)));
        file_info_text.push(Line::from(format!("Modified: {}", info.modified_time)));
        file_info_text.push(Line::from(format!("Features: {}", info.feature_count)));
        for (geom_type, count) in &info.geometry_counts {
            file_info_text.push(Line::from(format!("  {}: {}", geom_type, count)));
        }
        if let Some(bbox) = info.bbox {
            file_info_text.push(Line::from(format!(
                "BBox: [{:.2},{:.2},{:.2},{:.2}]",
                bbox[0], bbox[1], bbox[2], bbox[3]
            )));
        } else {
            file_info_text.push(Line::from(String::from("BBox: Not applicable/Found")));
        }
        if let Some(ref error) = info.parse_error {
            file_info_text.push(Line::from(format!("Error: {}", error)).fg(Color::Red));
        }
    } else {
        file_info_text.push(Line::from("Loading file info...".to_string()).fg(Color::Gray));
        file_info_text
            .push(Line::from("Or no file selected/available.".to_string()).fg(Color::Gray));
    }
    let file_info_paragraph = Paragraph::new(file_info_text)
        .block(file_info_block)
        .wrap(Wrap { trim: false });
    frame.render_widget(file_info_paragraph, right_panel_chunks[0]);

    // Section 2: Plotting Configuration Options
    let plotting_options_block = Block::default()
        .title(" Plotting Options ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::LightYellow));

    let inner_plotting_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // For "Next Color"
            Constraint::Length(1), // Points Visible
            Constraint::Length(1), // Lines Visible
            Constraint::Length(1), // Polygons Visible
            Constraint::Length(1), // Spacer (only one spacer now)
            Constraint::Length(1), // For Output Filename label and input
            Constraint::Min(0),    // Any remaining space for padding within the block
        ])
        .split(plotting_options_block.inner(right_panel_chunks[1]));

    let mut current_inner_chunk_idx = 0;

    // "Next Color" line
    let next_plot_color = &app.plot_colors[app.current_color_index_for_assignment];
    let next_color_line = Line::from(format!(
        "Next Color: R{} G{} B{}",
        next_plot_color.0, next_plot_color.1, next_plot_color.2
    ));
    frame.render_widget(
        Paragraph::new(next_color_line),
        inner_plotting_layout[current_inner_chunk_idx],
    );
    current_inner_chunk_idx += 1;

    // Toggles for visibility
    frame.render_widget(
        Paragraph::new(format!(
            "Points Visible: {}",
            if app.plot_points { "Yes" } else { "No" }
        )),
        inner_plotting_layout[current_inner_chunk_idx],
    );
    current_inner_chunk_idx += 1;

    frame.render_widget(
        Paragraph::new(format!(
            "Lines Visible: {}",
            if app.plot_lines { "Yes" } else { "No" }
        )),
        inner_plotting_layout[current_inner_chunk_idx],
    );
    current_inner_chunk_idx += 1;

    frame.render_widget(
        Paragraph::new(format!(
            "Polygons Visible: {}",
            if app.plot_polygons { "Yes" } else { "No" }
        )),
        inner_plotting_layout[current_inner_chunk_idx],
    );
    current_inner_chunk_idx += 1;

    // Spacer
    frame.render_widget(
        Paragraph::new(""),
        inner_plotting_layout[current_inner_chunk_idx],
    );
    current_inner_chunk_idx += 1;

    // Output Filename Field
    let output_filename_layout = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(17), // "Output Filename:"
            Constraint::Min(0),     // Input field
        ])
        .split(inner_plotting_layout[current_inner_chunk_idx]);
    current_inner_chunk_idx += 1; // Increment after splitting

    let filename_label = Paragraph::new("Output Filename:");
    frame.render_widget(filename_label, output_filename_layout[0]);

    // Removed borders from filename input paragraph.
    let filename_input_paragraph = Paragraph::new(app.output_filename_buffer.clone()).style(
        if app.current_mode == AppMode::EditingFilename {
            Style::default().fg(Color::White).bg(Color::Blue)
        } else {
            Style::default().fg(Color::White)
        },
    );
    frame.render_widget(filename_input_paragraph, output_filename_layout[1]);
    frame.render_widget(plotting_options_block, right_panel_chunks[1]);

    // Section 3: Dynamic Help / Keybinds
    let help_block = Block::default()
        .title(" Help / Keybinds ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::LightCyan));

    let help_lines: Vec<Line> = app
        .help_keybinds
        .iter()
        .map(|s| Line::from(s.clone()))
        .collect();
    let help_paragraph = Paragraph::new(help_lines)
        .block(help_block)
        .wrap(Wrap { trim: false });
    frame.render_widget(help_paragraph, right_panel_chunks[2]);

    // divider for resizing the main panels
    let divider_x_pos = main_content_layout[0].x + main_content_layout[0].width;
    for y in main_content_layout[0].y..(main_content_layout[0].y + main_content_layout[0].height) {
        let style = if app.is_resizing {
            Style::default().bg(Color::LightRed)
        } else {
            Style::default().bg(Color::DarkGray)
        };
        frame
            .buffer_mut()
            .get_mut(divider_x_pos, y)
            .set_symbol("│")
            .set_style(style);
    }
}

/// Renders a common footer area.
fn render_footer(frame: &mut Frame, app: &mut App, area: ratatui::layout::Rect) {
    let current_screen_name = match app.current_screen {
        CurrentScreen::Main => "Main",
        CurrentScreen::Help => "Help",
        CurrentScreen::GeoJsonMapper => "GeoJSON Mapper",
    };

    let current_mode_name = match app.current_mode {
        AppMode::Navigation => "Navigation",
        AppMode::EditingFilename => "Editing Filename",
        AppMode::Searching => "Searching",
    };

    let footer_text = Line::from(vec![
        Span::raw("Screen: "),
        Span::styled(
            current_screen_name,
            Style::default()
                .fg(Color::LightBlue)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(" | Mode: "),
        Span::styled(
            current_mode_name,
            Style::default()
                .fg(Color::LightMagenta)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(" | Press "),
        Span::styled(
            "q",
            Style::default().add_modifier(Modifier::BOLD).fg(Color::Red),
        ),
        Span::raw(" to quit "),
        Span::raw(" | Press "),
        Span::styled(
            "h",
            Style::default()
                .add_modifier(Modifier::BOLD)
                .fg(Color::Green),
        ),
        Span::raw(" for Help "),
    ]);

    let block = Block::default()
        .borders(Borders::TOP)
        .border_style(Style::default().fg(Color::DarkGray));

    let footer = Paragraph::new(footer_text)
        .alignment(Alignment::Center)
        .block(block)
        .style(Style::default().fg(Color::Gray));

    frame.render_widget(footer, area);
}
