use geojson::{Feature, GeoJson, Geometry, Value};
use plotters::prelude::*;
use std::error::Error;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

const GEOJSON_DIR: &str = "data/geojson/";
const OUTPUT_DIR: &str = "output/";

fn read_geojson(filepath: &str) -> Result<GeoJson, Box<dyn Error>> {
    let file = fs::File::open(filepath)?;
    let reader = io::BufReader::new(file);
    let geojson = GeoJson::from_reader(reader)?;
    Ok(geojson)
}

fn main() -> Result<(), Box<dyn Error>> {
    fs::create_dir_all(OUTPUT_DIR)?;

    let mut geojson_files: Vec<String> = Vec::new();
    let path = Path::new(GEOJSON_DIR);

    if !path.exists() {
        return Err(format!("Error: GeoJSON data directory not found at '{}'. Please ensure the 'natural-earth-vector-master' submodule is initialized and up-to-date.", GEOJSON_DIR).into());
    }
    if !path.is_dir() {
        return Err(format!("Error: Path '{}' is not a directory.", GEOJSON_DIR).into());
    }

    println!("\nAvailable GeoJSON files in {}:", GEOJSON_DIR);
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
        return Err(format!(
            "No .geojson files found in '{}'. Please ensure your GeoJSON data is correctly placed.",
            GEOJSON_DIR
        )
        .into());
    }

    geojson_files.sort(); // Sort alphabetically

    for (i, file_name) in geojson_files.iter().enumerate() {
        println!("{}. {}", i + 1, file_name);
    }

    let chosen_index;
    loop {
        print!("\nEnter the number of the GeoJSON file to plot: ");
        io::stdout().flush()?;

        let mut input = String::new();
        io::stdin().read_line(&mut input)?;
        let input = input.trim();

        match input.parse::<usize>() {
            Ok(num) if num > 0 && num <= geojson_files.len() => {
                chosen_index = num - 1; // Convert to 0-based index
                break;
            }
            _ => {
                println!(
                    "Invalid input. Please enter a number between 1 and {}.",
                    geojson_files.len()
                );
            }
        }
    }

    let chosen_filename_str = &geojson_files[chosen_index];
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

    match read_geojson(
        full_filepath
            .to_str()
            .expect("Failed to convert path to string"),
    ) {
        Ok(geojson) => {
            if let GeoJson::FeatureCollection(collection) = geojson {
                for feature in collection.features {
                    if let Some(geometry) = feature.geometry {
                        match geometry.value {
                            Value::LineString(lines) => {
                                chart.draw_series(LineSeries::new(
                                    lines
                                        .into_iter()
                                        .map(|line_coord| (line_coord[0], line_coord[1])),
                                    &RGBColor(0, 0, 0), // Black lines
                                ))?;
                            }
                            Value::MultiLineString(multi_lines) => {
                                for lines_segment in multi_lines {
                                    chart.draw_series(LineSeries::new(
                                        lines_segment
                                            .into_iter()
                                            .map(|line_coord| (line_coord[0], line_coord[1])),
                                        &RGBColor(0, 0, 0), // Black lines
                                    ))?;
                                }
                            }
                            Value::Polygon(polygon_rings) => {
                                // Draw the exterior ring of the polygon
                                if let Some(exterior_ring) = polygon_rings.get(0) {
                                    chart.draw_series(LineSeries::new(
                                        exterior_ring.into_iter().map(|point| (point[0], point[1])),
                                        &RGBColor(0, 0, 0), // Black outlines for polygons
                                    ))?;
                                }
                            }
                            Value::MultiPolygon(multi_polygon) => {
                                // For each polygon in the multi-polygon, draw its exterior ring
                                for polygon in multi_polygon {
                                    if let Some(exterior_ring) = polygon.get(0) {
                                        chart.draw_series(LineSeries::new(
                                            exterior_ring
                                                .into_iter()
                                                .map(|point| (point[0], point[1])),
                                            &RGBColor(0, 0, 0), // Black outlines for polygons
                                        ))?;
                                    }
                                }
                            }
                            _ => { /* Ignore other geometry types for now */ }
                        }
                    }
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

    Ok(())
}
