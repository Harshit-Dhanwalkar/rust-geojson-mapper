// main.rs for rust-geojson-mapper with ncurses TUI
use geojson::{GeoJson, Value};
use plotters::prelude::*;
use std::collections::HashMap;
use std::error::Error;
use std::fs;
use std::io::{self};
use std::path::{Path, PathBuf};

use ncurses::*;
use std::cmp;
use std::process;
use std::sync::atomic::{AtomicBool, Ordering};

static CTRLC: AtomicBool = AtomicBool::new(false);

extern "C" fn callback(_signum: i32) {
    CTRLC.store(true, Ordering::Relaxed);
}

fn init_signal_handler() {
    unsafe {
        if libc::signal(libc::SIGINT, callback as libc::sighandler_t) == libc::SIG_ERR {
            unreachable!()
        }
    }
}

fn poll_signal() -> bool {
    CTRLC.swap(false, Ordering::Relaxed)
}

const REGULAR_PAIR: i16 = 0;
const HIGHLIGHT_PAIR: i16 = 1;

#[derive(Default, Copy, Clone)]
struct Vec2 {
    x: i32,
    y: i32,
}

impl std::ops::Add for Vec2 {
    type Output = Vec2;

    fn add(self, rhs: Vec2) -> Vec2 {
        Vec2 {
            x: self.x + rhs.x,
            y: self.y + rhs.y,
        }
    }
}

impl std::ops::Mul for Vec2 {
    type Output = Vec2;

    fn mul(self, rhs: Vec2) -> Vec2 {
        Vec2 {
            x: self.x * rhs.x,
            y: self.y * rhs.y,
        }
    }
}

impl Vec2 {
    fn new(x: i32, y: i32) -> Self {
        Self { x, y }
    }
}

enum LayoutKind {
    Vert,
    Horz,
}

struct Layout {
    kind: LayoutKind,
    pos: Vec2,
    size: Vec2,
}

impl Layout {
    fn available_pos(&self) -> Vec2 {
        use LayoutKind::*;
        match self.kind {
            Horz => self.pos + self.size * Vec2::new(1, 0),
            Vert => self.pos + self.size * Vec2::new(0, 1),
        }
    }

    fn add_widget(&mut self, size: Vec2) {
        use LayoutKind::*;
        match self.kind {
            Horz => {
                self.size.x += size.x;
                self.size.y = cmp::max(self.size.y, size.y);
            }
            Vert => {
                self.size.x = cmp::max(self.size.x, size.x);
                self.size.y += size.y;
            }
        }
    }
}

#[derive(Default)]
struct Ui {
    layouts: Vec<Layout>,
    key: Option<i32>,
}

impl Ui {
    fn begin(&mut self, pos: Vec2, kind: LayoutKind) {
        assert!(self.layouts.is_empty());
        self.layouts.push(Layout {
            kind,
            pos,
            size: Vec2::new(0, 0),
        })
    }

    fn begin_layout(&mut self, kind: LayoutKind) {
        let layout = self
            .layouts
            .last()
            .expect("Can't create a layout outside of Ui::begin() and Ui::end()");
        let pos = layout.available_pos();
        self.layouts.push(Layout {
            kind,
            pos,
            size: Vec2::new(0, 0),
        });
    }

    fn end_layout(&mut self) {
        let layout = self
            .layouts
            .pop()
            .expect("Unbalanced Ui::begin_layout() and Ui::end_layout() calls.");
        self.layouts
            .last_mut()
            .expect("Unbalanced Ui::begin_layout() and Ui::end_layout() calls.")
            .add_widget(layout.size);
    }

    fn label_fixed_width(&mut self, text: &str, width: i32, pair: i16) {
        let layout = self
            .layouts
            .last_mut()
            .expect("Trying to render label outside of any layout");
        let pos = layout.available_pos();

        mv(pos.y, pos.x);
        attron(COLOR_PAIR(pair));
        addstr(text);
        attroff(COLOR_PAIR(pair));

        layout.add_widget(Vec2::new(width, 1));
    }

    fn edit_field(
        &mut self,
        buffer: &mut String,
        cursor: &mut usize,
        width: i32,
        key_pressed: Option<i32>,
    ) {
        let layout = self
            .layouts
            .last_mut()
            .expect("Trying to render edit field outside of any layout");
        let pos = layout.available_pos();

        if *cursor > buffer.len() {
            *cursor = buffer.len();
        }

        if let Some(key) = key_pressed {
            match key {
                32..=126 => {
                    // Printable ASCII characters
                    if *cursor >= buffer.len() {
                        buffer.push(key as u8 as char);
                    } else {
                        buffer.insert(*cursor, key as u8 as char);
                    }
                    *cursor += 1;
                }
                constants::KEY_LEFT => {
                    if *cursor > 0 {
                        *cursor -= 1
                    }
                }
                constants::KEY_RIGHT => {
                    if *cursor < buffer.len() {
                        *cursor += 1;
                    }
                }
                constants::KEY_BACKSPACE => {
                    if *cursor > 0 {
                        *cursor -= 1;
                        if *cursor < buffer.len() {
                            buffer.remove(*cursor);
                        }
                    }
                }
                constants::KEY_DC => {
                    // Delete key
                    if *cursor < buffer.len() {
                        buffer.remove(*cursor);
                    }
                }
                _ => {
                    // Do not consume other keys like Enter, Escape, which will be handled by the main loop
                }
            }
        }

        // Buffer
        {
            mv(pos.y, pos.x);
            attron(COLOR_PAIR(REGULAR_PAIR));
            addstr(buffer);
            for _ in buffer.len() as i32..width {
                addch(' ' as chtype);
            }
            attroff(COLOR_PAIR(REGULAR_PAIR));
            layout.add_widget(Vec2::new(width, 1));
        }

        // Cursor
        {
            mv(pos.y, pos.x + *cursor as i32);
            attron(COLOR_PAIR(HIGHLIGHT_PAIR));
            addstr(buffer.get(*cursor..=*cursor).unwrap_or(" "));
            attroff(COLOR_PAIR(HIGHLIGHT_PAIR));
        }
    }

    fn end(&mut self) {
        self.layouts
            .pop()
            .expect("Unbalanced Ui::begin() and Ui::end() calls.");
    }
}

const GEOJSON_DIR: &str = "data/geojson/";
const OUTPUT_DIR: &str = "output/";

fn read_geojson(filepath: &str) -> Result<GeoJson, Box<dyn Error>> {
    let file = fs::File::open(filepath)?;
    let reader = io::BufReader::new(file);
    let geojson = GeoJson::from_reader(reader)?;
    Ok(geojson)
}

#[derive(Default, Clone)]
struct GeoJsonInfo {
    file_size_kb: u64,
    modified_time: String,
    feature_count: usize,
    geometry_counts: HashMap<String, usize>,
    bbox: Option<[f64; 4]>, // [min_lon, min_lat, max_lon, max_lat]
    parse_error: Option<String>,
}

#[derive(PartialEq)]
enum AppMode {
    Navigation,
    EditingFilename,
    Searching,
}

// Basic fuzzy matching function
fn fuzzy_match(pattern: &str, text: &str) -> bool {
    if pattern.is_empty() {
        return true; // Empty pattern matches everything
    }

    let pattern_lower = pattern.to_lowercase(); // Case-insensitive search
    let text_lower = text.to_lowercase(); // Case-insensitive search

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
            return false; // Character not found in sequence
        }
        current_pattern_char = pattern_chars.next();
    }
    true
}

fn main() -> Result<(), Box<dyn Error>> {
    init_signal_handler();

    fs::create_dir_all(OUTPUT_DIR)?;

    let mut geojson_files: Vec<String> = Vec::new();
    let path = Path::new(GEOJSON_DIR);

    if !path.exists() {
        eprintln!(
            "Error: GeoJSON data directory not found at '{}'. Please ensure the 'natural-earth-vector-master' submodule is initialized and up-to-date.",
            GEOJSON_DIR
        );
        process::exit(1);
    }
    if !path.is_dir() {
        eprintln!("Error: Path '{}' is not a directory.", GEOJSON_DIR);
        process::exit(1);
    }

    for entry in fs::read_dir(path)? {
        let entry = entry?;
        let entry_path = entry.path();
        if entry_path.is_file() {
            if let Some(extension) = entry_path.extension() {
                if extension == "geojson" {
                    if let Some(file_name) = entry_path.file_name() {
                        if let Some(name_str) = file_name.to_str() {
                            geojson_files.push(name_str.to_string());
                        }
                    }
                }
            }
        }
    }

    if geojson_files.is_empty() {
        eprintln!(
            "No .geojson files found in '{}'. Please ensure your GeoJSON data is correctly placed.",
            GEOJSON_DIR
        );
        process::exit(1);
    }

    geojson_files.sort(); // Sort alphabetically

    // --- ncurses TUI Setup ---
    initscr();
    noecho();
    keypad(stdscr(), true);
    timeout(16); // ~60 FPS
    curs_set(CURSOR_VISIBILITY::CURSOR_INVISIBLE);

    start_color();
    init_pair(REGULAR_PAIR, COLOR_WHITE, COLOR_BLACK);
    init_pair(HIGHLIGHT_PAIR, COLOR_BLACK, COLOR_WHITE);

    // Define a palette of colors for plotting
    let plot_colors = [
        RGBColor(0, 0, 0),     // Black
        RGBColor(255, 0, 0),   // Red
        RGBColor(0, 255, 0),   // Green
        RGBColor(0, 0, 255),   // Blue
        RGBColor(255, 255, 0), // Yellow
        RGBColor(255, 0, 255), // Magenta
        RGBColor(0, 255, 255), // Cyan
    ];

    let mut ui = Ui::default();
    let mut selected_file_index: usize = 0;
    let mut scroll_offset: usize = 0;
    let mut quit = false;
    let mut notification = String::from("Select GeoJSON files to plot:");

    // Plotting options state
    let mut plot_points = true;
    let mut plot_lines = true;
    let mut plot_polygons = true;

    let mut selected_files_status: Vec<bool> = vec![false; geojson_files.len()];
    let mut assigned_plot_colors: Vec<Option<RGBColor>> = vec![None; geojson_files.len()];
    let mut current_color_index_for_assignment = 0; // Index for cycling colors when selecting files

    // Output filename state
    let mut output_filename_buffer = String::from("combined_plot.png");
    let mut output_filename_cursor: usize = 0;
    let mut previous_output_filename_buffer = String::new(); // To revert on Escape

    let mut current_mode = AppMode::Navigation; // Initial mode

    // Fuzzy search state
    let mut search_query_buffer = String::new();
    let mut search_query_cursor: usize = 0;
    let mut filtered_geojson_indices: Vec<usize> = (0..geojson_files.len()).collect(); // All files initially
    let mut previous_search_query_buffer = String::new(); // To revert on Escape

    let mut help_keybinds = vec![
        "J/K or Arrow Keys: Navigate file list",
        "Space: Toggle file selection",
        "Enter: Plot selected files",
        "C: Cycle next assignment color",
        "R: Rename output plot",
        "/: Start fuzzy search",
        "P: Toggle Points visibility",
        "L: Toggle Lines visibility",
        "O: Toggle Polygons visibility",
        "Q: Quit the application",
    ];

    // Caching for GeoJSON metadata
    let mut cached_geojson_info: Vec<Option<GeoJsonInfo>> = vec![None; geojson_files.len()];
    let mut previous_selected_file_index_in_filtered: usize = selected_file_index;

    // Main TUI Loop
    while !quit && !poll_signal() {
        erase(); // Clear screen

        let mut x = 0;
        let mut y = 0;
        getmaxyx(stdscr(), &mut y, &mut x); // Get current terminal dimensions

        // Re-filter files if search query changed or just entered/exited search mode
        if current_mode == AppMode::Searching
            || search_query_buffer.ne(&previous_search_query_buffer)
        {
            filtered_geojson_indices.clear();
            if search_query_buffer.is_empty() {
                for i in 0..geojson_files.len() {
                    filtered_geojson_indices.push(i);
                }
            } else {
                for (i, filename) in geojson_files.iter().enumerate() {
                    if fuzzy_match(&search_query_buffer, filename) {
                        filtered_geojson_indices.push(i);
                    }
                }
            }
            // Reset selected index if the current one is no longer in the filtered list
            if filtered_geojson_indices.is_empty() {
                selected_file_index = 0;
            } else if selected_file_index >= filtered_geojson_indices.len() {
                selected_file_index = filtered_geojson_indices.len() - 1;
            }
            previous_search_query_buffer.clone_from(&search_query_buffer);
        }

        // If not in search mode and a search was active, reset notification
        if current_mode != AppMode::Searching
            && !search_query_buffer.is_empty()
            && notification.starts_with("Searching for")
        {
            notification = String::from("Select GeoJSON files to plot:");
        }

        let mut header_rows = 2; // Notification + spacer
        if let AppMode::Searching = current_mode {
            header_rows += 1;
        }
        let footer_rows = 0;
        let title_row = 1;
        let fixed_ui_rows = header_rows + footer_rows + title_row;

        let max_visible_items_in_list = cmp::max(0, y - fixed_ui_rows) as usize;

        // Adjust scroll_offset to keep selected_file_index in view
        let current_list_len = filtered_geojson_indices.len();
        if selected_file_index >= current_list_len {
            selected_file_index = cmp::max(0, current_list_len.saturating_sub(1));
        }

        if selected_file_index >= scroll_offset + max_visible_items_in_list
            && max_visible_items_in_list > 0
        {
            scroll_offset = selected_file_index - max_visible_items_in_list + 1;
        }
        if selected_file_index < scroll_offset {
            scroll_offset = selected_file_index;
        }
        if current_list_len <= max_visible_items_in_list {
            scroll_offset = 0; // All files fit, no scrolling needed
        } else if scroll_offset > current_list_len.saturating_sub(max_visible_items_in_list) {
            scroll_offset = current_list_len.saturating_sub(max_visible_items_in_list);
        }

        let main_content_width = x; // Total width for the main content area
        let left_panel_width = main_content_width / 2;
        let right_panel_width = main_content_width - left_panel_width; // Right panel takes remaining width

        let available_content_rows = max_visible_items_in_list + 1; // +1 for "Available GeoJSON files:" title

        let section_title_rows = 1;
        let num_right_sections = 3;
        let total_right_section_title_rows = section_title_rows * num_right_sections;

        let mut base_section_content_height = 0;
        if available_content_rows > total_right_section_title_rows {
            base_section_content_height =
                (available_content_rows - total_right_section_title_rows) / num_right_sections;
        }
        let remaining_rows_for_last_section =
            (available_content_rows - total_right_section_title_rows) % num_right_sections;

        // --- File Information & Metadata Retrieval (Cached) ---
        let current_original_file_index = if filtered_geojson_indices.is_empty() {
            0 // Or use a value indicating no file selected
        } else {
            filtered_geojson_indices[selected_file_index]
        };

        let current_geojson_info: GeoJsonInfo;
        if current_original_file_index != previous_selected_file_index_in_filtered
            || cached_geojson_info[current_original_file_index].is_none()
        {
            // Load and process only if index changed or not cached yet
            let mut info = GeoJsonInfo::default();
            if let Some(chosen_filename_str) = geojson_files.get(current_original_file_index) {
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
                                        // Each ring (exterior and interior)
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
                                _ => { /* Ignore GeometryCollection or unknown types for BBox for now */
                                }
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
                                // Handle case where root is just a geometry, treat as one feature
                                info.feature_count = 1;
                                process_geometry_for_info(&geometry);
                            }
                        }

                        if info.feature_count > 0 && min_lon != f64::MAX {
                            // Check if bounding box was actually calculated
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
            cached_geojson_info[current_original_file_index] = Some(info.clone()); // Store in cache
            current_geojson_info = info; // Return the newly loaded info
        } else {
            current_geojson_info = cached_geojson_info[current_original_file_index]
                .as_ref()
                .unwrap()
                .clone(); // Get from cache
        }

        let mut file_info_lines: Vec<String> = Vec::new();
        file_info_lines.push(format!("Size: {} KB", current_geojson_info.file_size_kb));
        file_info_lines.push(format!("Modified: {}", current_geojson_info.modified_time));
        file_info_lines.push(format!("Features: {}", current_geojson_info.feature_count));
        for (geom_type, count) in &current_geojson_info.geometry_counts {
            file_info_lines.push(format!("  {}: {}", geom_type, count));
        }
        if let Some(bbox) = current_geojson_info.bbox {
            file_info_lines.push(format!(
                "BBox: [{:.2},{:.2},{:.2},{:.2}]",
                bbox[0], bbox[1], bbox[2], bbox[3]
            ));
        } else {
            file_info_lines.push(String::from("BBox: Not applicable/Found"));
        }
        if let Some(ref error) = current_geojson_info.parse_error {
            file_info_lines.push(error.clone());
        }

        ui.begin(Vec2::new(0, 0), LayoutKind::Vert);
        {
            ui.label_fixed_width(&notification, x, REGULAR_PAIR);
            ui.label_fixed_width("", x, REGULAR_PAIR); // Spacer

            // Render search bar if in search mode
            if let AppMode::Searching = current_mode {
                ui.begin_layout(LayoutKind::Horz);
                {
                    ui.label_fixed_width("Search:", 8, REGULAR_PAIR);
                    ui.edit_field(
                        &mut search_query_buffer,
                        &mut search_query_cursor,
                        x - 8,
                        ui.key,
                    );
                }
                ui.end_layout();
            }

            ui.begin_layout(LayoutKind::Horz); // Start Horizontal split for main content
            {
                // --- Left Panel: GeoJSON File List ---
                ui.begin_layout(LayoutKind::Vert);
                {
                    ui.label_fixed_width(
                        "Available GeoJSON files:",
                        left_panel_width,
                        REGULAR_PAIR,
                    );
                    // Display visible file list items from filtered_geojson_indices
                    let end_display_index = cmp::min(
                        scroll_offset + max_visible_items_in_list,
                        filtered_geojson_indices.len(),
                    );
                    for i in scroll_offset..end_display_index {
                        let original_index = filtered_geojson_indices[i];
                        let file_name = &geojson_files[original_index];
                        let selection_indicator = if selected_files_status[original_index] {
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
                        let pair = if i == selected_file_index {
                            HIGHLIGHT_PAIR
                        } else {
                            REGULAR_PAIR
                        };
                        ui.label_fixed_width(&display_text, left_panel_width, pair);
                    }
                    // Fill remaining lines with empty spaces if there are fewer files than max_visible_items
                    for _ in (end_display_index - scroll_offset)..max_visible_items_in_list {
                        ui.label_fixed_width("", left_panel_width, REGULAR_PAIR);
                    }
                }
                ui.end_layout(); // End Left Panel

                // --- Right Panel: Three Vertical Sections ---
                ui.begin_layout(LayoutKind::Vert);
                {
                    // Section 1: Detailed File Information
                    ui.begin_layout(LayoutKind::Vert);
                    {
                        ui.label_fixed_width(
                            "--- File Information ---",
                            right_panel_width,
                            REGULAR_PAIR,
                        );
                        for line in file_info_lines.iter() {
                            ui.label_fixed_width(line, right_panel_width, REGULAR_PAIR);
                        }
                        // Fill remaining lines for this section's content
                        let lines_printed = file_info_lines.len();
                        let lines_to_fill =
                            cmp::max(0, base_section_content_height.saturating_sub(lines_printed));
                        for _ in 0..lines_to_fill {
                            ui.label_fixed_width("", right_panel_width, REGULAR_PAIR);
                        }
                    }
                    ui.end_layout();

                    // Section 2: Dynamic Help / Keybinds
                    ui.begin_layout(LayoutKind::Vert);
                    {
                        ui.label_fixed_width(
                            "--- Help / Keybinds ---",
                            right_panel_width,
                            REGULAR_PAIR,
                        );
                        // Display help information
                        for line in help_keybinds.iter() {
                            ui.label_fixed_width(line, right_panel_width, REGULAR_PAIR);
                        }
                        // Fill remaining lines for this section's content
                        let lines_printed = help_keybinds.len();
                        let lines_to_fill =
                            cmp::max(0, base_section_content_height.saturating_sub(lines_printed));
                        for _ in 0..lines_to_fill {
                            ui.label_fixed_width("", right_panel_width, REGULAR_PAIR);
                        }
                    }
                    ui.end_layout();

                    // Section 3: Plotting Configuration Options
                    ui.begin_layout(LayoutKind::Vert);
                    {
                        ui.label_fixed_width(
                            "--- Plotting Options ---",
                            right_panel_width,
                            REGULAR_PAIR,
                        );
                        let next_plot_color = &plot_colors[current_color_index_for_assignment];
                        ui.label_fixed_width(
                            &format!(
                                "Next Color to Assign: R{} G{} B{}",
                                next_plot_color.0, next_plot_color.1, next_plot_color.2
                            ),
                            right_panel_width,
                            REGULAR_PAIR,
                        );

                        // Start a horizontal layout for the filename label and field
                        ui.begin_layout(LayoutKind::Horz);
                        {
                            let filename_label_width = 20; // Fixed width for the label
                            ui.label_fixed_width(
                                "Output Filename:",
                                filename_label_width,
                                REGULAR_PAIR,
                            );

                            let remaining_width_for_field =
                                right_panel_width - filename_label_width;

                            if let AppMode::EditingFilename = current_mode {
                                ui.edit_field(
                                    &mut output_filename_buffer,
                                    &mut output_filename_cursor,
                                    remaining_width_for_field - 2, // -2 for padding or cursor space
                                    ui.key,
                                );
                            } else {
                                ui.label_fixed_width(
                                    &output_filename_buffer,
                                    remaining_width_for_field,
                                    REGULAR_PAIR,
                                );
                            }
                        }
                        ui.end_layout(); // End horizontal layout for filename

                        // Display visibility toggles
                        ui.label_fixed_width(
                            &format!("Points Visible: {}", if plot_points { "Yes" } else { "No" }),
                            right_panel_width,
                            REGULAR_PAIR,
                        );
                        ui.label_fixed_width(
                            &format!("Lines Visible: {}", if plot_lines { "Yes" } else { "No" }),
                            right_panel_width,
                            REGULAR_PAIR,
                        );
                        ui.label_fixed_width(
                            &format!(
                                "Polygons Visible: {}",
                                if plot_polygons { "Yes" } else { "No" }
                            ),
                            right_panel_width,
                            REGULAR_PAIR,
                        );
                        // Fill remaining lines for this section's content
                        let lines_printed = 4;
                        let lines_to_fill = cmp::max(
                            0,
                            (base_section_content_height + remaining_rows_for_last_section)
                                .saturating_sub(lines_printed),
                        );
                        for _ in 0..lines_to_fill {
                            ui.label_fixed_width("", right_panel_width, REGULAR_PAIR);
                        }
                    }
                    ui.end_layout();
                }
                ui.end_layout(); // End Right Panel
            }
            ui.end_layout(); // End Horizontal split for main content

            ui.label_fixed_width("", x, REGULAR_PAIR); // Spacer
        }
        ui.end();

        refresh(); // Update screen

        let key = getch(); // Get user input
        ui.key = Some(key); // Pass the raw key to ui.edit_field if in editing mode

        if key != ERR {
            notification.clear(); // Clear notification on new input

            match current_mode {
                AppMode::Navigation => {
                    match key {
                        // Navigation
                        constants::KEY_DOWN => {
                            if selected_file_index + 1 < filtered_geojson_indices.len() {
                                selected_file_index += 1;
                            }
                        }
                        // Use ASCII value for 'j'
                        106 => {
                            // 'j' as i32
                            if selected_file_index + 1 < filtered_geojson_indices.len() {
                                selected_file_index += 1;
                            }
                        }
                        constants::KEY_UP => {
                            if selected_file_index > 0 {
                                selected_file_index -= 1;
                            }
                        }
                        // Use ASCII value for 'k'
                        107 => {
                            // 'k' as i32
                            if selected_file_index > 0 {
                                selected_file_index -= 1;
                            }
                        }
                        // Toggle file selection
                        32 => {
                            // ' ' as i32 (space)
                            if !filtered_geojson_indices.is_empty() {
                                let original_index = filtered_geojson_indices[selected_file_index];
                                selected_files_status[original_index] =
                                    !selected_files_status[original_index];
                                if selected_files_status[original_index] {
                                    assigned_plot_colors[original_index] =
                                        Some(plot_colors[current_color_index_for_assignment]);
                                    notification = format!(
                                        "Selected: {} (Color: R{} G{} B{})",
                                        geojson_files[original_index],
                                        plot_colors[current_color_index_for_assignment].0,
                                        plot_colors[current_color_index_for_assignment].1,
                                        plot_colors[current_color_index_for_assignment].2
                                    );
                                    current_color_index_for_assignment =
                                        (current_color_index_for_assignment + 1)
                                            % plot_colors.len();
                                } else {
                                    assigned_plot_colors[original_index] = None;
                                    notification =
                                        format!("Deselected: {}", geojson_files[original_index]);
                                }
                            } else {
                                notification = String::from("No files to select in current view.");
                            }
                        }
                        // Plot selected files
                        constants::KEY_ENTER | 10 => {
                            // 10 is '\n' as i32
                            let num_selected = selected_files_status.iter().filter(|&&s| s).count();
                            if num_selected > 0 {
                                quit = true; // Exit loop to process selection
                                notification =
                                    format!("Plotting {} selected files...", num_selected);
                            } else {
                                notification =
                                    String::from("No files selected to plot. Use Space to select.");
                            }
                        }
                        // Cycle next assignment color
                        99 | 67 => {
                            // 'c' as i32 | 'C' as i32
                            current_color_index_for_assignment =
                                (current_color_index_for_assignment + 1) % plot_colors.len();
                            notification = format!(
                                "Next assignment color set to R{} G{} B{}",
                                plot_colors[current_color_index_for_assignment].0,
                                plot_colors[current_color_index_for_assignment].1,
                                plot_colors[current_color_index_for_assignment].2
                            );
                        }
                        // Rename output plot
                        114 | 82 => {
                            // 'r' as i32 | 'R' as i32
                            current_mode = AppMode::EditingFilename;
                            previous_output_filename_buffer.clone_from(&output_filename_buffer);
                            notification = String::from(
                                "Editing filename. Press Enter to confirm, Escape to cancel.",
                            );
                            curs_set(CURSOR_VISIBILITY::CURSOR_VISIBLE); // Show cursor
                        }
                        // Start fuzzy search
                        47 => {
                            // '/' as i32
                            current_mode = AppMode::Searching;
                            previous_search_query_buffer.clone_from(&search_query_buffer); // Save current for potential revert
                            notification = String::from(
                                "Enter search query. Press Enter to apply, Escape to cancel.",
                            );
                            curs_set(CURSOR_VISIBILITY::CURSOR_VISIBLE); // Show cursor
                        }
                        // Toggle Points
                        112 | 80 => {
                            // 'p' as i32 | 'P' as i32
                            plot_points = !plot_points;
                            notification = format!(
                                "Points visibility: {}",
                                if plot_points { "ON" } else { "OFF" }
                            );
                        }
                        // Toggle Lines
                        108 | 76 => {
                            // 'l' as i32 | 'L' as i32
                            plot_lines = !plot_lines;
                            notification = format!(
                                "Lines visibility: {}",
                                if plot_lines { "ON" } else { "OFF" }
                            );
                        }
                        // Toggle Polygons (using 'o' to avoid conflict with 'P' for points)
                        111 | 79 => {
                            // 'o' as i32 | 'O' as i32
                            plot_polygons = !plot_polygons;
                            notification = format!(
                                "Polygons visibility: {}",
                                if plot_polygons { "ON" } else { "OFF" }
                            );
                        }
                        // Quit
                        113 => {
                            // 'q' as i32
                            quit = true;
                            notification = String::from("Exiting...");
                        }
                        _ => {
                            // Handle other keys or do nothing
                            notification = format!("Unknown key: {}", key);
                        }
                    }
                }
                AppMode::EditingFilename => {
                    match key {
                        constants::KEY_ENTER | 10 => {
                            // 10 is '\n' as i32
                            if output_filename_buffer.is_empty() {
                                notification = String::from("Filename cannot be empty. Reverted.");
                                output_filename_buffer.clone_from(&previous_output_filename_buffer);
                            } else if !output_filename_buffer.ends_with(".png")
                                && !output_filename_buffer.ends_with(".jpg")
                                && !output_filename_buffer.ends_with(".jpeg")
                                && !output_filename_buffer.ends_with(".bmp")
                            {
                                notification = String::from(
                                    "Filename must end with .png, .jpg, .jpeg, or .bmp. Reverted.",
                                );
                                output_filename_buffer.clone_from(&previous_output_filename_buffer);
                            } else {
                                notification =
                                    format!("Output filename set to: {}", output_filename_buffer);
                            }
                            current_mode = AppMode::Navigation;
                            curs_set(CURSOR_VISIBILITY::CURSOR_INVISIBLE); // Hide cursor
                        }
                        constants::KEY_EXIT | constants::KEY_CANCEL | 27 => {
                            // 27 is ASCII for ESC
                            output_filename_buffer.clone_from(&previous_output_filename_buffer);
                            notification =
                                String::from("Filename editing cancelled. Reverted to previous.");
                            current_mode = AppMode::Navigation;
                            curs_set(CURSOR_VISIBILITY::CURSOR_INVISIBLE); // Hide cursor
                        }
                        _ => {}
                    }
                }
                AppMode::Searching => {
                    match key {
                        constants::KEY_ENTER | 10 => {
                            // 10 is '\n' as i32
                            if search_query_buffer.is_empty() {
                                notification = String::from("Search cleared. Showing all files.");
                            } else {
                                notification = format!(
                                    "Searching for: '{}' ({} results)",
                                    search_query_buffer,
                                    filtered_geojson_indices.len()
                                );
                            }
                            current_mode = AppMode::Navigation;
                            curs_set(CURSOR_VISIBILITY::CURSOR_INVISIBLE); // Hide cursor
                        }
                        constants::KEY_EXIT | constants::KEY_CANCEL | 27 => {
                            // 27 is ASCII for ESC
                            search_query_buffer.clone_from(&previous_search_query_buffer); // Revert search query
                            // Re-filter immediately to show original list if query was cleared
                            filtered_geojson_indices.clear();
                            for i in 0..geojson_files.len() {
                                filtered_geojson_indices.push(i);
                            }
                            selected_file_index = 0; // Reset selection to top of the (now full) list
                            notification = String::from("Search cancelled. Showing all files.");
                            current_mode = AppMode::Navigation;
                            curs_set(CURSOR_VISIBILITY::CURSOR_INVISIBLE); // Hide cursor
                        }
                        _ => {
                            // `edit_field` handles text input, so no other direct key handling needed here for input.
                        }
                    }
                }
            }
        }
        previous_selected_file_index_in_filtered = current_original_file_index; // Update previous index after handling input
    }

    // --- Plotting Logic (after selection) ---
    endwin(); // End ncurses mode before plotting

    let files_to_plot: Vec<(usize, &String)> = geojson_files
        .iter()
        .enumerate()
        .filter(|(i, _)| selected_files_status[*i])
        .collect();

    if files_to_plot.is_empty() {
        println!("No files selected for plotting. Exited without generating a plot.");
    } else if !CTRLC.load(Ordering::Relaxed) {
        let output_filename = PathBuf::from(OUTPUT_DIR).join(&output_filename_buffer);

        // --- Calculate combined BBox for selected files ---
        let mut overall_min_lon = f64::MAX;
        let mut overall_min_lat = f64::MAX;
        let mut overall_max_lon = f64::MIN;
        let mut overall_max_lat = f64::MIN;
        let mut bbox_found = false;

        for (file_idx, _) in &files_to_plot {
            if let Some(info) = &cached_geojson_info[*file_idx] {
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
                .expect("Failed to convert output path to string"),
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
            let plot_color_for_file = assigned_plot_colors[file_idx].unwrap_or_else(|| {
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
                                        plot_points,
                                        plot_lines,
                                        plot_polygons,
                                    )?;
                                }
                            }
                        }
                        GeoJson::Feature(feature) => {
                            if let Some(geometry) = feature.geometry {
                                draw_geometry(
                                    geometry,
                                    &plot_color_for_file,
                                    plot_points,
                                    plot_lines,
                                    plot_polygons,
                                )?;
                            }
                        }
                        GeoJson::Geometry(geometry) => {
                            draw_geometry(
                                geometry,
                                &plot_color_for_file,
                                plot_points,
                                plot_lines,
                                plot_polygons,
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
    } else {
        println!("Exited without plotting.");
    }

    Ok(())
}
