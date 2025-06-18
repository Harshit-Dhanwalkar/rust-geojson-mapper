// main.rs
use chrono;
use crossterm::{
    event::{DisableMouseCapture, EnableMouseCapture, KeyCode, MouseButton, MouseEventKind},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use geojson::{GeoJson, Value};
use plotters::prelude::*;
use ratatui::{Terminal, backend::CrosstermBackend};
use std::cmp;
use std::collections::HashMap;
use std::{
    error::Error,
    fs, io,
    path::{Path, PathBuf},
    time::Duration,
};

mod app;
mod event;
mod ui;

use app::{App, AppMode, CurrentScreen, GeoJsonInfo, TerminalEvent};
use event::{Event, EventHandler};

const GEOJSON_DIR: &str = "data/geojson/";
const OUTPUT_DIR: &str = "output/";

// Helper function to read GeoJSON
fn read_geojson(filepath: &str) -> Result<GeoJson, Box<dyn Error>> {
    let file = fs::File::open(filepath)?;
    let reader = io::BufReader::new(file);
    let geojson = GeoJson::from_reader(reader)?;
    Ok(geojson)
}

// Basic fuzzy matching function
fn fuzzy_match(pattern: &str, text: &str) -> bool {
    if pattern.is_empty() {
        return true;
    }

    let pattern_lower = pattern.to_lowercase();
    let text_lower = text.to_lowercase();

    let mut pattern_chars = pattern_lower.chars().peekable();
    let mut text_chars = text_lower.chars();
    let mut current_pattern_char = pattern_chars.next();

    while let Some(p_char) = current_pattern_char {
        let mut found_char = false;
        while let Some(t_char) = text_chars.next() {
            if t_char == p_char {
                found_char = true;
                break;
            }
        }
        if !found_char {
            return false;
        }
        current_pattern_char = pattern_chars.next();
    }
    true
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Ensure output directory exists
    fs::create_dir_all(OUTPUT_DIR)?;

    // --- Terminal Setup ---
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // --- Initialize Application State ---
    let mut app = App::new();

    // Load GeoJSON file names
    let mut geojson_files_loaded: Vec<String> = Vec::new();
    let path = Path::new(GEOJSON_DIR);

    if !path.exists() || !path.is_dir() {
        eprintln!(
            "Error: GeoJSON data directory not found or not a directory at '{}'.",
            GEOJSON_DIR
        );
        // Fallback or exit if data is critical
    } else {
        for entry in fs::read_dir(path)? {
            let entry = entry?;
            let entry_path = entry.path();
            if entry_path.is_file() {
                if let Some(extension) = entry_path.extension() {
                    if extension == "geojson" {
                        if let Some(file_name) = entry_path.file_name() {
                            if let Some(name_str) = file_name.to_str() {
                                geojson_files_loaded.push(name_str.to_string());
                            }
                        }
                    }
                }
            }
        }
        geojson_files_loaded.sort(); // Sort alphabetically
    }

    if geojson_files_loaded.is_empty() {
        app.notification =
            String::from("No .geojson files found in data/geojson/. Please add some.");
    }

    app.setup_geojson_data(geojson_files_loaded);

    // --- Initialize Event Handler ---
    let tick_rate = Duration::from_millis(250);
    let event_handler = EventHandler::new(tick_rate);

    // --- Main TUI Loop ---
    let mut quit_app = false; // Separate flag to break main loop for plotting
    while !quit_app {
        // --- Pre-rendering state updates ---

        // Re-filter files if search query changed or just entered/exited search mode
        let prev_search_query = app.previous_search_query_buffer.clone();
        if app.current_mode == AppMode::Searching || app.search_query_buffer.ne(&prev_search_query)
        {
            app.filtered_geojson_indices.clear();
            if app.search_query_buffer.is_empty() {
                for i in 0..app.geojson_files.len() {
                    app.filtered_geojson_indices.push(i);
                }
            } else {
                for (i, filename) in app.geojson_files.iter().enumerate() {
                    if fuzzy_match(&app.search_query_buffer, filename) {
                        app.filtered_geojson_indices.push(i);
                    }
                }
            }
            if app.filtered_geojson_indices.is_empty() {
                app.selected_file_index = 0;
            } else {
                app.selected_file_index = cmp::min(
                    app.selected_file_index,
                    app.filtered_geojson_indices.len().saturating_sub(1),
                );
            }
            app.previous_search_query_buffer
                .clone_from(&app.search_query_buffer);
        }

        // Adjust scroll_offset to keep selected_file_index in view
        let current_list_len = app.filtered_geojson_indices.len();
        let estimated_max_visible_items = terminal.size()?.height.saturating_sub(5) as usize;
        if app.selected_file_index >= app.scroll_offset + estimated_max_visible_items
            && estimated_max_visible_items > 0
        {
            app.scroll_offset = app.selected_file_index - estimated_max_visible_items + 1;
        }
        if app.selected_file_index < app.scroll_offset {
            app.scroll_offset = app.selected_file_index;
        }
        if current_list_len <= estimated_max_visible_items {
            app.scroll_offset = 0; // All files fit, no scrolling needed
        } else if app.scroll_offset > current_list_len.saturating_sub(estimated_max_visible_items) {
            app.scroll_offset = current_list_len.saturating_sub(estimated_max_visible_items);
        }

        // --- Cache GeoJSON Info for selected file ---
        let current_original_file_index = if app.filtered_geojson_indices.is_empty() {
            0
        } else {
            app.filtered_geojson_indices[app.selected_file_index]
        };

        if current_original_file_index != app.previous_selected_file_index_in_filtered
            || app.cached_geojson_info[current_original_file_index].is_none()
        {
            let mut info = GeoJsonInfo::default();
            if let Some(chosen_filename_str) = app.geojson_files.get(current_original_file_index) {
                let full_filepath = PathBuf::from(GEOJSON_DIR).join(chosen_filename_str);
                if let Ok(metadata) = fs::metadata(&full_filepath) {
                    info.file_size_kb = metadata.len() / 1024;
                    if let Ok(time) = metadata.modified() {
                        let datetime: chrono::DateTime<chrono::Local> = time.into();
                        info.modified_time = format!("{}", datetime.format("%Y-%m-%d %H:%M"));
                    } else {
                        info.modified_time = String::from("N/A");
                    }
                } else {
                    info.parse_error = Some(String::from("File info: Not available"));
                }

                match read_geojson(
                    full_filepath
                        .to_str()
                        .expect("Failed to convert path to string"),
                ) {
                    Ok(geojson) => {
                        let mut min_lon = f64::MAX;
                        let mut min_lat = f64::MAX;
                        let mut max_lon = f64::MIN;
                        let mut max_lat = f64::MIN;

                        let mut process_geometry_for_info = |geometry: &geojson::Geometry| {
                            let geom_type = geometry.value.type_name().to_string();
                            *info.geometry_counts.entry(geom_type).or_insert(0) += 1;

                            match &geometry.value {
                                Value::Point(c) => {
                                    min_lon = min_lon.min(c[0]);
                                    min_lat = min_lat.min(c[1]);
                                    max_lon = max_lon.max(c[0]);
                                    max_lat = max_lat.max(c[1]);
                                }
                                Value::MultiPoint(coords_vec) => {
                                    for c in coords_vec {
                                        min_lon = min_lon.min(c[0]);
                                        min_lat = min_lat.min(c[1]);
                                        max_lon = max_lon.max(c[0]);
                                        max_lat = max_lat.max(c[1]);
                                    }
                                }
                                Value::LineString(line) => {
                                    for c in line {
                                        min_lon = min_lon.min(c[0]);
                                        min_lat = min_lat.min(c[1]);
                                        max_lon = max_lon.max(c[0]);
                                        max_lat = max_lat.max(c[1]);
                                    }
                                }
                                Value::MultiLineString(multi_line) => {
                                    for line in multi_line {
                                        for c in line {
                                            min_lon = min_lon.min(c[0]);
                                            min_lat = min_lat.min(c[1]);
                                            max_lon = max_lon.max(c[0]);
                                            max_lat = max_lat.max(c[1]);
                                        }
                                    }
                                }
                                Value::Polygon(polygon) => {
                                    for ring in polygon {
                                        for c in ring {
                                            min_lon = min_lon.min(c[0]);
                                            min_lat = min_lat.min(c[1]);
                                            max_lon = max_lon.max(c[0]);
                                            max_lat = max_lat.max(c[1]);
                                        }
                                    }
                                }
                                Value::MultiPolygon(multi_polygon) => {
                                    for polygon in multi_polygon {
                                        for ring in polygon {
                                            for c in ring {
                                                min_lon = min_lon.min(c[0]);
                                                min_lat = min_lat.min(c[1]);
                                                max_lon = max_lon.max(c[0]);
                                                max_lat = max_lat.max(c[1]);
                                            }
                                        }
                                    }
                                }
                                _ => {}
                            }
                        };

                        match geojson {
                            GeoJson::FeatureCollection(collection) => {
                                info.feature_count = collection.features.len();
                                for feature in collection.features {
                                    if let Some(geometry) = feature.geometry {
                                        process_geometry_for_info(&geometry);
                                    }
                                }
                            }
                            GeoJson::Feature(feature) => {
                                info.feature_count = 1;
                                if let Some(geometry) = feature.geometry {
                                    process_geometry_for_info(&geometry);
                                }
                            }
                            GeoJson::Geometry(geometry) => {
                                info.feature_count = 1;
                                process_geometry_for_info(&geometry);
                            }
                        }

                        if info.feature_count > 0 && min_lon != f64::MAX {
                            info.bbox = Some([min_lon, min_lat, max_lon, max_lat]);
                        }
                    }
                    Err(e) => {
                        info.parse_error = Some(format!("GeoJSON Parse Error: {}", e));
                    }
                }
            } else {
                info.parse_error = Some(String::from("Info: No file selected"));
            }
            app.cached_geojson_info[current_original_file_index] = Some(info);
            app.previous_selected_file_index_in_filtered = current_original_file_index;
        }

        // --- Draw UI ---
        terminal.draw(|f| ui::render(f, &mut app))?;

        // --- Handle Events ---
        if let Some(event) = event_handler.next(tick_rate)? {
            match event {
                Event::Input(key_event) => {
                    app.notification.clear(); // Clear notification on new input

                    match app.current_mode {
                        AppMode::Navigation => {
                            match key_event.code {
                                KeyCode::Down | KeyCode::Char('j') => {
                                    if app.selected_file_index + 1
                                        < app.filtered_geojson_indices.len()
                                    {
                                        app.selected_file_index += 1;
                                    }
                                }
                                KeyCode::Up | KeyCode::Char('k') => {
                                    if app.selected_file_index > 0 {
                                        app.selected_file_index -= 1;
                                    }
                                }
                                KeyCode::Char(' ') => {
                                    // Space
                                    if !app.filtered_geojson_indices.is_empty() {
                                        let original_index =
                                            app.filtered_geojson_indices[app.selected_file_index];
                                        app.selected_files_status[original_index] =
                                            !app.selected_files_status[original_index];
                                        if app.selected_files_status[original_index] {
                                            app.assigned_plot_colors[original_index] = Some(
                                                app.plot_colors
                                                    [app.current_color_index_for_assignment],
                                            );
                                            app.notification = format!(
                                                "Selected: {} (Color: R{} G{} B{})",
                                                app.geojson_files[original_index],
                                                app.plot_colors
                                                    [app.current_color_index_for_assignment]
                                                    .0,
                                                app.plot_colors
                                                    [app.current_color_index_for_assignment]
                                                    .1,
                                                app.plot_colors
                                                    [app.current_color_index_for_assignment]
                                                    .2
                                            );
                                            app.current_color_index_for_assignment =
                                                (app.current_color_index_for_assignment + 1)
                                                    % app.plot_colors.len();
                                        } else {
                                            app.assigned_plot_colors[original_index] = None;
                                            app.notification = format!(
                                                "Deselected: {}",
                                                app.geojson_files[original_index]
                                            );
                                        }
                                    } else {
                                        app.notification =
                                            String::from("No files to select in current view.");
                                    }
                                }
                                KeyCode::Enter => {
                                    let num_selected =
                                        app.selected_files_status.iter().filter(|&&s| s).count();
                                    if num_selected > 0 {
                                        quit_app = true; // Exit loop to process selection
                                        app.notification =
                                            format!("Plotting {} selected files...", num_selected);
                                    } else {
                                        app.notification = String::from(
                                            "No files selected to plot. Use Space to select.",
                                        );
                                    }
                                }
                                KeyCode::Char('c') | KeyCode::Char('C') => {
                                    app.current_color_index_for_assignment =
                                        (app.current_color_index_for_assignment + 1)
                                            % app.plot_colors.len();
                                    app.notification = format!(
                                        "Next assignment color set to R{} G{} B{}",
                                        app.plot_colors[app.current_color_index_for_assignment].0,
                                        app.plot_colors[app.current_color_index_for_assignment].1,
                                        app.plot_colors[app.current_color_index_for_assignment].2
                                    );
                                }
                                KeyCode::Char('r') | KeyCode::Char('R') => {
                                    app.current_mode = AppMode::EditingFilename;
                                    app.previous_output_filename_buffer
                                        .clone_from(&app.output_filename_buffer);
                                    app.notification = String::from(
                                        "Editing filename. Press Enter to confirm, Escape to cancel.",
                                    );
                                }
                                KeyCode::Char('/') => {
                                    app.current_mode = AppMode::Searching;
                                    app.previous_search_query_buffer
                                        .clone_from(&app.search_query_buffer);
                                    app.notification = String::from(
                                        "Enter search query. Press Enter to apply, Escape to cancel.",
                                    );
                                }
                                KeyCode::Char('p') | KeyCode::Char('P') => {
                                    app.plot_points = !app.plot_points;
                                    app.notification = format!(
                                        "Points visibility: {}",
                                        if app.plot_points { "ON" } else { "OFF" }
                                    );
                                }
                                KeyCode::Char('l') | KeyCode::Char('L') => {
                                    app.plot_lines = !app.plot_lines;
                                    app.notification = format!(
                                        "Lines visibility: {}",
                                        if app.plot_lines { "ON" } else { "OFF" }
                                    );
                                }
                                KeyCode::Char('o') | KeyCode::Char('O') => {
                                    app.plot_polygons = !app.plot_polygons;
                                    app.notification = format!(
                                        "Polygons visibility: {}",
                                        if app.plot_polygons { "ON" } else { "OFF" }
                                    );
                                }
                                KeyCode::Char('q') | KeyCode::Char('Q') => {
                                    quit_app = true;
                                    app.notification = String::from("Exiting...");
                                }
                                KeyCode::Char('h') | KeyCode::Char('H') => {
                                    app.current_screen = CurrentScreen::Help;
                                    app.notification = String::from("Showing Help screen.");
                                }
                                _ => { /* Ignore other key events */ }
                            }
                        }
                        AppMode::EditingFilename => {
                            match key_event.code {
                                KeyCode::Enter => {
                                    if app.output_filename_buffer.is_empty() {
                                        app.notification =
                                            String::from("Filename cannot be empty. Reverted.");
                                        app.output_filename_buffer
                                            .clone_from(&app.previous_output_filename_buffer);
                                    } else if !app.output_filename_buffer.ends_with(".png")
                                        && !app.output_filename_buffer.ends_with(".jpg")
                                        && !app.output_filename_buffer.ends_with(".jpeg")
                                        && !app.output_filename_buffer.ends_with(".bmp")
                                    {
                                        app.notification = String::from(
                                            "Filename must end with .png, .jpg, .jpeg, or .bmp. Reverted.",
                                        );
                                        app.output_filename_buffer
                                            .clone_from(&app.previous_output_filename_buffer);
                                    } else {
                                        app.notification = format!(
                                            "Output filename set to: {}",
                                            app.output_filename_buffer
                                        );
                                    }
                                    app.current_mode = AppMode::Navigation;
                                }
                                KeyCode::Esc => {
                                    // Escape key
                                    app.output_filename_buffer
                                        .clone_from(&app.previous_output_filename_buffer);
                                    app.notification = String::from(
                                        "Filename editing cancelled. Reverted to previous.",
                                    );
                                    app.current_mode = AppMode::Navigation;
                                }
                                KeyCode::Backspace => {
                                    if app.output_filename_cursor > 0 {
                                        app.output_filename_cursor -= 1;
                                        if app.output_filename_cursor
                                            < app.output_filename_buffer.len()
                                        {
                                            app.output_filename_buffer
                                                .remove(app.output_filename_cursor);
                                        }
                                    }
                                }
                                KeyCode::Delete => {
                                    if app.output_filename_cursor < app.output_filename_buffer.len()
                                    {
                                        app.output_filename_buffer
                                            .remove(app.output_filename_cursor);
                                    }
                                }
                                KeyCode::Left => {
                                    if app.output_filename_cursor > 0 {
                                        app.output_filename_cursor -= 1;
                                    }
                                }
                                KeyCode::Right => {
                                    if app.output_filename_cursor < app.output_filename_buffer.len()
                                    {
                                        app.output_filename_cursor += 1;
                                    }
                                }
                                KeyCode::Char(c) => {
                                    if app.output_filename_cursor
                                        >= app.output_filename_buffer.len()
                                    {
                                        app.output_filename_buffer.push(c);
                                    } else {
                                        app.output_filename_buffer
                                            .insert(app.output_filename_cursor, c);
                                    }
                                    app.output_filename_cursor += 1;
                                }
                                _ => {}
                            }
                        }
                        AppMode::Searching => {
                            match key_event.code {
                                KeyCode::Enter => {
                                    if app.search_query_buffer.is_empty() {
                                        app.notification =
                                            String::from("Search cleared. Showing all files.");
                                    } else {
                                        app.notification = format!(
                                            "Searching for: '{}' ({} results)",
                                            app.search_query_buffer,
                                            app.filtered_geojson_indices.len()
                                        );
                                    }
                                    app.current_mode = AppMode::Navigation;
                                }
                                KeyCode::Esc => {
                                    // Escape key
                                    app.search_query_buffer
                                        .clone_from(&app.previous_search_query_buffer);
                                    app.current_mode = AppMode::Navigation;
                                    app.notification =
                                        String::from("Search cancelled. Showing all files.");
                                }
                                KeyCode::Backspace => {
                                    if app.search_query_cursor > 0 {
                                        app.search_query_cursor -= 1;
                                        if app.search_query_cursor < app.search_query_buffer.len() {
                                            app.search_query_buffer.remove(app.search_query_cursor);
                                        }
                                    }
                                }
                                KeyCode::Delete => {
                                    if app.search_query_cursor < app.search_query_buffer.len() {
                                        app.search_query_buffer.remove(app.search_query_cursor);
                                    }
                                }
                                KeyCode::Left => {
                                    if app.search_query_cursor > 0 {
                                        app.search_query_cursor -= 1;
                                    }
                                }
                                KeyCode::Right => {
                                    if app.search_query_cursor < app.search_query_buffer.len() {
                                        app.search_query_cursor += 1;
                                    }
                                }
                                KeyCode::Char(c) => {
                                    if app.search_query_cursor >= app.search_query_buffer.len() {
                                        app.search_query_buffer.push(c);
                                    } else {
                                        app.search_query_buffer.insert(app.search_query_cursor, c);
                                    }
                                    app.search_query_cursor += 1;
                                }
                                _ => {}
                            }
                        }
                    }
                }
                Event::TerminalEvent(TerminalEvent::Resize) => {
                    // ratatui handles resize redrawing automatically
                }
                Event::Tick => {
                    // Periodic updates
                }
                Event::Mouse(mouse_event) => {
                    // Resizing logic GeoJsonMapper screen
                    if app.current_screen == CurrentScreen::GeoJsonMapper {
                        let terminal_width = terminal.size()?.width;
                        // Calculate divider position based on current app.left_pane_width_percentage
                        let divider_col = (terminal_width as f64
                            * (app.left_pane_width_percentage as f64 / 100.0))
                            as u16;

                        match mouse_event.kind {
                            MouseEventKind::Down(MouseButton::Left) => {
                                // Check if mouse click is near the divider (within a small range)
                                if mouse_event.column >= divider_col.saturating_sub(1)
                                    && mouse_event.column <= divider_col.saturating_add(1)
                                {
                                    app.is_resizing = true;
                                }
                            }
                            MouseEventKind::Drag(MouseButton::Left) => {
                                if app.is_resizing {
                                    if terminal_width > 0 {
                                        let new_width_percent = (mouse_event.column as f64
                                            / terminal_width as f64)
                                            * 100.0;
                                        // Clamp to a reasonable range
                                        app.left_pane_width_percentage =
                                            (new_width_percent.round() as u16).clamp(10, 90);
                                    }
                                }
                            }
                            MouseEventKind::Up(MouseButton::Left) => {
                                app.is_resizing = false;
                            }
                            _ => {} // Ignore other mouse events
                        }
                    }
                }
            }
        }
    }

    // --- Plotting Logic (after TUI loop exits via Enter) ---
    execute!(terminal.backend_mut(), DisableMouseCapture)?; // Disable mouse capture here
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    let files_to_plot: Vec<(usize, &String)> = app
        .geojson_files
        .iter()
        .enumerate()
        .filter(|(i, _)| app.selected_files_status[*i])
        .collect();

    if files_to_plot.is_empty() {
        println!("No files selected for plotting. Exited without generating a plot.");
    } else {
        let output_filename = PathBuf::from(OUTPUT_DIR).join(&app.output_filename_buffer);

        // --- Calculate combined BBox for selected files ---
        let mut overall_min_lon = f64::MAX;
        let mut overall_min_lat = f64::MAX;
        let mut overall_max_lon = f64::MIN;
        let mut overall_max_lat = f64::MIN;
        let mut bbox_found = false;

        for (file_idx, _) in &files_to_plot {
            if let Some(info) = &app.cached_geojson_info[*file_idx] {
                if let Some(bbox) = info.bbox {
                    overall_min_lon = overall_min_lon.min(bbox[0]);
                    overall_min_lat = overall_min_lat.min(bbox[1]);
                    overall_max_lon = overall_max_lon.max(bbox[2]);
                    overall_max_lat = overall_max_lat.max(bbox[3]);
                    bbox_found = true;
                }
            }
        }

        let mut x_range = -180.0f64..180.0f64;
        let mut y_range = -90.0f64..90.0f64;

        if bbox_found {
            let padding_percentage = 0.1; // 10% padding
            let epsilon = 0.001;

            let mut lon_range = overall_max_lon - overall_min_lon;
            if lon_range < epsilon {
                lon_range = epsilon;
            }
            let mut lat_range = overall_max_lat - overall_min_lat;
            if lat_range < epsilon {
                lat_range = epsilon;
            }

            let lon_padding = lon_range * padding_percentage;
            let lat_padding = lat_range * padding_percentage;

            let padded_min_lon = (overall_min_lon - lon_padding).max(-180.0);
            let padded_max_lon = (overall_max_lon + lon_padding).min(180.0);
            let padded_min_lat = (overall_min_lat - lat_padding).max(-90.0);
            let padded_max_lat = (overall_max_lat + lat_padding).min(90.0);

            x_range = padded_min_lon..padded_max_lon;
            y_range = padded_min_lat..padded_max_lat;
        } else {
            println!(
                "Warning: No valid bounding box found for selected files. Using default global view."
            );
        }

        // Setup drawing area only if files are selected and not quitting
        let chart_caption = format!("GeoJSON Plot");

        let width = 1024;
        let height = 768;
        let root = BitMapBackend::new(
            output_filename
                .to_str()
                .expect("Failed to convert path to string"),
            (width, height),
        )
        .into_drawing_area();
        root.fill(&RGBColor(173, 216, 230))?; // Light blue ocean background

        let mut chart = ChartBuilder::on(&root)
            .margin(10)
            .caption(&chart_caption, ("sans-serif", 40).into_font())
            .build_cartesian_2d(x_range, y_range)?;

        chart.configure_mesh().draw()?;

        for (file_idx, chosen_filename_str) in files_to_plot {
            let full_filepath = PathBuf::from(GEOJSON_DIR).join(chosen_filename_str);
            let plot_color_for_file = app.assigned_plot_colors[file_idx].unwrap_or_else(|| {
                // Fallback to black if for some reason color wasn't assigned
                RGBColor(0, 0, 0)
            });

            match read_geojson(
                full_filepath
                    .to_str()
                    .expect("Failed to convert path to string"),
            ) {
                Ok(geojson) => {
                    let mut draw_geometry = |geometry: geojson::Geometry,
                                             color: &RGBColor,
                                             plot_points_flag: bool,
                                             plot_lines_flag: bool,
                                             plot_polygons_flag: bool|
                     -> Result<(), Box<dyn Error>> {
                        match geometry.value {
                            Value::Point(c) => {
                                if plot_points_flag {
                                    chart.draw_series(PointSeries::of_element(
                                        vec![(c[0], c[1])],
                                        5, // Point size
                                        color.filled(),
                                        &|c, s, st| {
                                            return EmptyElement::at(c)
                                                + Circle::new((0, 0), s, st);
                                        },
                                    ))?;
                                }
                            }
                            Value::MultiPoint(coords_vec) => {
                                if plot_points_flag {
                                    chart.draw_series(PointSeries::of_element(
                                        coords_vec.into_iter().map(|c| (c[0], c[1])),
                                        5,
                                        color.filled(),
                                        &|c, s, st| {
                                            return EmptyElement::at(c)
                                                + Circle::new((0, 0), s, st);
                                        },
                                    ))?;
                                }
                            }
                            Value::LineString(lines) => {
                                if plot_lines_flag {
                                    chart.draw_series(LineSeries::new(
                                        lines
                                            .into_iter()
                                            .map(|line_coord| (line_coord[0], line_coord[1])),
                                        color,
                                    ))?;
                                }
                            }
                            Value::MultiLineString(multi_lines) => {
                                if plot_lines_flag {
                                    for lines_segment in multi_lines {
                                        chart.draw_series(LineSeries::new(
                                            lines_segment
                                                .into_iter()
                                                .map(|line_coord| (line_coord[0], line_coord[1])),
                                            color,
                                        ))?;
                                    }
                                }
                            }
                            Value::Polygon(polygon_rings) => {
                                if plot_polygons_flag {
                                    // Draw the exterior ring of the polygon
                                    if let Some(exterior_ring) = polygon_rings.get(0) {
                                        chart.draw_series(LineSeries::new(
                                            exterior_ring
                                                .into_iter()
                                                .map(|point| (point[0], point[1])),
                                            color,
                                        ))?;
                                    }
                                }
                            }
                            Value::MultiPolygon(multi_polygon) => {
                                if plot_polygons_flag {
                                    for polygon in multi_polygon {
                                        if let Some(exterior_ring) = polygon.get(0) {
                                            chart.draw_series(LineSeries::new(
                                                exterior_ring
                                                    .into_iter()
                                                    .map(|point| (point[0], point[1])),
                                                color,
                                            ))?;
                                        }
                                    }
                                }
                            }
                            _ => {}
                        }
                        Ok(())
                    };

                    match geojson {
                        GeoJson::FeatureCollection(collection) => {
                            for feature in collection.features {
                                if let Some(geometry) = feature.geometry {
                                    draw_geometry(
                                        geometry,
                                        &plot_color_for_file,
                                        app.plot_points,
                                        app.plot_lines,
                                        app.plot_polygons,
                                    )?;
                                }
                            }
                        }
                        GeoJson::Feature(feature) => {
                            if let Some(geometry) = feature.geometry {
                                draw_geometry(
                                    geometry,
                                    &plot_color_for_file,
                                    app.plot_points,
                                    app.plot_lines,
                                    app.plot_polygons,
                                )?;
                            }
                        }
                        GeoJson::Geometry(geometry) => {
                            draw_geometry(
                                geometry,
                                &plot_color_for_file,
                                app.plot_points,
                                app.plot_lines,
                                app.plot_polygons,
                            )?;
                        }
                    }
                }
                Err(e) => eprintln!(
                    "Error reading GeoJSON from {}: {}",
                    full_filepath.display(),
                    e
                ),
            }
        }

        root.present()?;
        println!("Combined plot generated to {}", output_filename.display());
    }

    Ok(())
}
