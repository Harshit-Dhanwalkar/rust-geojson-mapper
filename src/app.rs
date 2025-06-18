// app.rs

use plotters::prelude::RGBColor;
use std::collections::HashMap; // For plot colors

#[derive(PartialEq)]
pub enum CurrentScreen {
    Main,
    Help,
    GeoJsonMapper,
}

#[derive(PartialEq)]
pub enum AppMode {
    Navigation,
    EditingFilename,
    Searching,
}

#[derive(Debug, Clone, Copy)]
pub enum TerminalEvent {
    Resize,
}

// Struct to hold cached GeoJSON file information
#[derive(Default, Clone)]
pub struct GeoJsonInfo {
    pub file_size_kb: u64,
    pub modified_time: String,
    pub feature_count: usize,
    pub geometry_counts: HashMap<String, usize>,
    pub bbox: Option<[f64; 4]>, // [min_lon, min_lat, max_lon, max_lat]
    pub parse_error: Option<String>,
}

pub struct App {
    pub current_screen: CurrentScreen,
    pub current_mode: AppMode, // Current operational mode of the TUI

    // State related to GeoJSON files
    pub geojson_files: Vec<String>,
    pub selected_file_index: usize, // Index in `filtered_geojson_indices`
    pub scroll_offset: usize,       // Scroll position for the file list
    pub selected_files_status: Vec<bool>, // Selection status for all original files
    pub assigned_plot_colors: Vec<Option<RGBColor>>, // Assigned colors for plotting
    pub current_color_index_for_assignment: usize, // Index for cycling colors

    // Plotting options
    pub plot_points: bool,
    pub plot_lines: bool,
    pub plot_polygons: bool,

    // Output filename editing
    pub output_filename_buffer: String,
    pub output_filename_cursor: usize,
    pub previous_output_filename_buffer: String,

    // Fuzzy search
    pub search_query_buffer: String,
    pub search_query_cursor: usize,
    pub filtered_geojson_indices: Vec<usize>, // Indices into `geojson_files`
    pub previous_search_query_buffer: String,

    // Cached GeoJSON metadata
    pub cached_geojson_info: Vec<Option<GeoJsonInfo>>,
    pub previous_selected_file_index_in_filtered: usize,

    // UI related
    pub notification: String,
    pub help_keybinds: Vec<String>,

    // Plotting colors
    pub plot_colors: [RGBColor; 7],

    // Resizing for main GeoJSON Mapper UI
    pub left_pane_width_percentage: u16, // Width of the left (file list) pane
    pub is_resizing: bool,               // True when actively dragging the divider
}

impl App {
    /// Constructs a new `App` with initial states.
    pub fn new() -> App {
        App {
            current_screen: CurrentScreen::GeoJsonMapper, // Start directly in the GeoJSON Mapper UI
            current_mode: AppMode::Navigation,

            geojson_files: Vec::new(),
            selected_file_index: 0,
            scroll_offset: 0,
            selected_files_status: Vec::new(),
            assigned_plot_colors: Vec::new(),
            current_color_index_for_assignment: 0,

            plot_points: true,
            plot_lines: true,
            plot_polygons: true,

            output_filename_buffer: String::from("combined_plot.png"),
            output_filename_cursor: 0,
            previous_output_filename_buffer: String::new(),

            search_query_buffer: String::new(),
            search_query_cursor: 0,
            filtered_geojson_indices: Vec::new(),
            previous_search_query_buffer: String::new(),

            cached_geojson_info: Vec::new(),
            previous_selected_file_index_in_filtered: 0,

            notification: String::from("Select GeoJSON files to plot:"),
            help_keybinds: vec![
                "J/K or Arrow Keys: Navigate file list".to_string(),
                "Space: Toggle file selection".to_string(),
                "Enter: Plot selected files".to_string(),
                "C: Cycle next assignment color".to_string(),
                "R: Rename output plot".to_string(),
                "/: Start fuzzy search".to_string(),
                "P: Toggle Points visibility".to_string(),
                "L: Toggle Lines visibility".to_string(),
                "O: Toggle Polygons visibility".to_string(),
                "Q: Quit the application".to_string(),
                "H: Show Help screen".to_string(),
                "Click & Drag Divider: Resize panels".to_string(),
            ],

            plot_colors: [
                RGBColor(0, 0, 0),     // Black
                RGBColor(255, 0, 0),   // Red
                RGBColor(0, 255, 0),   // Green
                RGBColor(0, 0, 255),   // Blue
                RGBColor(255, 255, 0), // Yellow
                RGBColor(255, 0, 255), // Magenta
                RGBColor(0, 255, 255), // Cyan
            ],

            left_pane_width_percentage: 50, // Default 50% width for left pane
            is_resizing: false,
        }
    }

    /// Sets up initial GeoJSON data
    pub fn setup_geojson_data(&mut self, geojson_files_input: Vec<String>) {
        self.geojson_files = geojson_files_input;
        let num_files = self.geojson_files.len();
        self.selected_files_status = vec![false; num_files];
        self.assigned_plot_colors = vec![None; num_files];
        self.cached_geojson_info = vec![None; num_files];
        self.filtered_geojson_indices = (0..num_files).collect(); // Initially all files are filtered
        self.selected_file_index = 0; // Reset selected index
    }
}
