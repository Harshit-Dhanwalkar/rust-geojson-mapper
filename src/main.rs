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

    fn edit_field(&mut self, buffer: &mut String, cursor: &mut usize, width: i32) {
        let layout = self
            .layouts
            .last_mut()
            .expect("Trying to render edit field outside of any layout");
        let pos = layout.available_pos();

        if *cursor > buffer.len() {
            *cursor = buffer.len();
        }

        if let Some(key) = self.key.take() {
            match key {
                32..=126 => {
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
                    if *cursor < buffer.len() {
                        buffer.remove(*cursor);
                    }
                }
                _ => {
                    self.key = Some(key);
                }
            }
        }

        // Buffer
        {
            mv(pos.y, pos.x);
            attron(COLOR_PAIR(REGULAR_PAIR));
            addstr(buffer);
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
    let mut current_color_index = 0;

    let mut ui = Ui::default();
    let mut selected_file_index: usize = 0;
    let mut scroll_offset: usize = 0;
    let mut quit = false;
    let mut notification = String::from("Select a GeoJSON file to plot:");

    let help_keybinds = vec![
        "J/K or Arrow Keys: Navigate file list",
        "Enter: Select highlighted file to plot",
        "C: Change plot color",
        "Q: Quit the application",
    ];

    // Caching for GeoJSON metadata
    let mut cached_geojson_info: Vec<Option<GeoJsonInfo>> = vec![None; geojson_files.len()];
    let mut previous_selected_file_index: usize = selected_file_index;

    // Main TUI Loop
    while !quit && !poll_signal() {
        erase(); // Clear screen

        let mut x = 0;
        let mut y = 0;
        getmaxyx(stdscr(), &mut y, &mut x); // Get current terminal dimensions

        // Calculate available rows for the file list
        let header_rows = 2; // Notification + first spacer
        let footer_rows = 2; // Second spacer + instructions
        let title_row = 1; // "Available GeoJSON files:" row, consumed by the main content area title
        let fixed_ui_rows = header_rows + footer_rows + title_row;

        let max_visible_items_in_list = cmp::max(0, y - fixed_ui_rows) as usize;

        // Adjust scroll_offset to keep selected_file_index in view
        if selected_file_index >= geojson_files.len() {
            selected_file_index = cmp::max(0, geojson_files.len().saturating_sub(1));
        }

        if selected_file_index >= scroll_offset + max_visible_items_in_list
            && max_visible_items_in_list > 0
        {
            scroll_offset = selected_file_index - max_visible_items_in_list + 1;
        }
        if selected_file_index < scroll_offset {
            scroll_offset = selected_file_index;
        }
        // Ensure scroll_offset doesn't go out of bounds at the end of the list
        if geojson_files.len() <= max_visible_items_in_list {
            scroll_offset = 0; // All files fit, no scrolling needed
        } else if scroll_offset
            > geojson_files
                .len()
                .saturating_sub(max_visible_items_in_list)
        {
            scroll_offset = geojson_files
                .len()
                .saturating_sub(max_visible_items_in_list);
        }

        let main_content_width = x; // Total width for the main content area
        let left_panel_width = main_content_width / 2;
        let right_panel_width = main_content_width - left_panel_width; // Right panel takes remaining width

        let available_content_rows = max_visible_items_in_list + 1; // +1 for "Available GeoJSON files:" title

        let section_title_rows = 1; // Each section title takes 1 row
        let num_right_sections = 3;
        let total_right_section_title_rows = section_title_rows * num_right_sections;

        let mut base_section_content_height = 0;
        if available_content_rows > total_right_section_title_rows {
            base_section_content_height =
                (available_content_rows - total_right_section_title_rows) / num_right_sections;
        }
        let remaining_rows_for_last_section =
            (available_content_rows - total_right_section_title_rows) % num_right_sections;

        // --- File Information & Metadata Retrieval ---
        let current_geojson_info: GeoJsonInfo;
        if selected_file_index != previous_selected_file_index
            || cached_geojson_info[selected_file_index].is_none()
        {
            // Load and process only if index changed or not cached yet
            let mut info = GeoJsonInfo::default();
            if let Some(chosen_filename_str) = geojson_files.get(selected_file_index) {
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
            cached_geojson_info[selected_file_index] = Some(info.clone()); // Store in cache
            current_geojson_info = info; // Return the newly loaded info
        } else {
            current_geojson_info = cached_geojson_info[selected_file_index]
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
                    // Display visible file list items
                    let end_display_index = cmp::min(
                        scroll_offset + max_visible_items_in_list,
                        geojson_files.len(),
                    );
                    for i in scroll_offset..end_display_index {
                        let file_name = &geojson_files[i];
                        let display_text = format!("{}. {}", i + 1, file_name);
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

                    // Section 2: Plotting Configuration Options
                    ui.begin_layout(LayoutKind::Vert);
                    {
                        ui.label_fixed_width(
                            "--- Plotting Options ---",
                            right_panel_width,
                            REGULAR_PAIR,
                        );
                        // Display current plot color
                        let current_plot_color = &plot_colors[current_color_index];
                        ui.label_fixed_width(
                            &format!(
                                "Current Plot Color: R{} G{} B{}",
                                current_plot_color.0, current_plot_color.1, current_plot_color.2
                            ),
                            right_panel_width,
                            REGULAR_PAIR,
                        );
                    }
                    ui.end_layout();
                }
                ui.end_layout(); // End Right Panel
            }
            ui.end_layout(); // End Horizontal split for main content

            ui.label_fixed_width("", x, REGULAR_PAIR); // Spacer
            ui.label_fixed_width(
                "Use J/K to navigate, Enter to select, C to change color, Q to quit.",
                x,
                REGULAR_PAIR,
            );
        }
        ui.end();

        refresh(); // Update screen

        let key = getch(); // Get user input
        if key != ERR {
            ui.key = Some(key);
            notification.clear(); // Clear notification on new input

            match key {
                // Navigation
                constants::KEY_DOWN => {
                    if selected_file_index + 1 < geojson_files.len() {
                        selected_file_index += 1;
                    }
                }
                val if val == 'j' as i32 => {
                    if selected_file_index + 1 < geojson_files.len() {
                        selected_file_index += 1;
                    }
                }
                constants::KEY_UP => {
                    if selected_file_index > 0 {
                        selected_file_index -= 1;
                    }
                }
                val if val == 'k' as i32 => {
                    if selected_file_index > 0 {
                        selected_file_index -= 1;
                    }
                }
                // Select file
                constants::KEY_ENTER => {
                    quit = true;
                    notification = format!("Selected: {}", geojson_files[selected_file_index]);
                }
                val if val == '\n' as i32 => {
                    quit = true;
                    notification = format!("Selected: {}", geojson_files[selected_file_index]);
                }
                // Change plot color
                val if val == 'c' as i32 || val == 'C' as i32 => {
                    current_color_index = (current_color_index + 1) % plot_colors.len();
                    notification = format!(
                        "Plot color changed to R{} G{} B{}",
                        plot_colors[current_color_index].0,
                        plot_colors[current_color_index].1,
                        plot_colors[current_color_index].2
                    );
                }
                // Quit
                val if val == 'q' as i32 => {
                    quit = true;
                    notification = String::from("Exiting...");
                }
                _ => {
                    // Handle other keys or do nothing
                    notification = format!("Unknown key: {}", key);
                }
            }
        }
        previous_selected_file_index = selected_file_index;
    }

    // --- Plotting Logic ---
    endwin(); // End ncurses mode before plotting

    if !CTRLC.load(Ordering::Relaxed) && ui.key.map(|k| k as u8 as char) != Some('q') {
        let chosen_filename_str = &geojson_files[selected_file_index];
        let full_filepath = PathBuf::from(GEOJSON_DIR).join(chosen_filename_str);

        let output_name_prefix = chosen_filename_str
            .strip_suffix(".geojson")
            .unwrap_or(chosen_filename_str);
        let output_filename = PathBuf::from(OUTPUT_DIR).join(format!("{}.png", output_name_prefix));
        let chart_caption = format!(
            "{} Map",
            output_name_prefix.replace("_", " ").to_uppercase()
        );

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
            .build_cartesian_2d(-180.0f64..180.0f64, -90.0f64..90.0f64)?;

        chart.configure_mesh().draw()?;

        // Get the current plotting color
        let current_plot_color = &plot_colors[current_color_index];

        match read_geojson(
            full_filepath
                .to_str()
                .expect("Failed to convert path to string"),
        ) {
            Ok(geojson) => {
                let mut draw_geometry = |geometry: geojson::Geometry,
                                         color: &RGBColor|
                 -> Result<(), Box<dyn Error>> {
                    match geometry.value {
                        Value::Point(c) => {
                            chart.draw_series(PointSeries::of_element(
                                vec![(c[0], c[1])],
                                5, // Point size
                                color.filled(),
                                &|c, s, st| {
                                    return EmptyElement::at(c) + Circle::new((0, 0), s, st);
                                },
                            ))?;
                        }
                        Value::MultiPoint(coords_vec) => {
                            chart.draw_series(PointSeries::of_element(
                                coords_vec.into_iter().map(|c| (c[0], c[1])),
                                5,
                                color.filled(),
                                &|c, s, st| {
                                    return EmptyElement::at(c) + Circle::new((0, 0), s, st);
                                },
                            ))?;
                        }
                        Value::LineString(lines) => {
                            chart.draw_series(LineSeries::new(
                                lines
                                    .into_iter()
                                    .map(|line_coord| (line_coord[0], line_coord[1])),
                                color,
                            ))?;
                        }
                        Value::MultiLineString(multi_lines) => {
                            for lines_segment in multi_lines {
                                chart.draw_series(LineSeries::new(
                                    lines_segment
                                        .into_iter()
                                        .map(|line_coord| (line_coord[0], line_coord[1])),
                                    color,
                                ))?;
                            }
                        }
                        Value::Polygon(polygon_rings) => {
                            // Draw the exterior ring of the polygon
                            if let Some(exterior_ring) = polygon_rings.get(0) {
                                chart.draw_series(LineSeries::new(
                                    exterior_ring.into_iter().map(|point| (point[0], point[1])),
                                    color,
                                ))?;
                            }
                        }
                        Value::MultiPolygon(multi_polygon) => {
                            for polygon in multi_polygon {
                                if let Some(exterior_ring) = polygon.get(0) {
                                    chart.draw_series(LineSeries::new(
                                        exterior_ring.into_iter().map(|point| (point[0], point[1])),
                                        color,
                                    ))?;
                                }
                            }
                        }
                        _ => { /* Ignore GeometryCollection for now */ }
                    }
                    Ok(())
                };

                match geojson {
                    GeoJson::FeatureCollection(collection) => {
                        for feature in collection.features {
                            if let Some(geometry) = feature.geometry {
                                draw_geometry(geometry, current_plot_color)?;
                            }
                        }
                    }
                    GeoJson::Feature(feature) => {
                        if let Some(geometry) = feature.geometry {
                            draw_geometry(geometry, current_plot_color)?;
                        }
                    }
                    GeoJson::Geometry(geometry) => {
                        draw_geometry(geometry, current_plot_color)?;
                    }
                }
            }
            Err(e) => eprintln!(
                "Error reading GeoJSON from {}: {}",
                full_filepath.display(),
                e
            ),
        }

        root.present()?;
        println!("Plot generated to {}", output_filename.display());
    } else {
        println!("Exited without plotting.");
    }

    Ok(())
}
