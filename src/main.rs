use geojson::{Feature, GeoJson, Geometry, Value};
use plotters::prelude::*;
use std::error::Error;
use std::fs::File;
use std::io::BufReader;

fn read_geojson(filepath: &str) -> Result<GeoJson, Box<dyn Error>> {
    let file = File::open(filepath)?;
    let reader = BufReader::new(file);
    let geojson = GeoJson::from_reader(reader)?;
    Ok(geojson)
}

fn main() -> Result<(), Box<dyn Error>> {
    let width = 1024;
    let height = 768;
    let root = BitMapBackend::new("world_coastlines.png", (width, height)).into_drawing_area();
    root.fill(&RGBColor(173, 216, 230))?; // Light blue ocean

    let mut chart = ChartBuilder::on(&root)
        .margin(10)
        .caption("World Coastlines", ("sans-serif", 40).into_font())
        .build_cartesian_2d(-180.0f64..180.0f64, -90.0f64..90.0f64)?;

    chart.configure_mesh().draw()?;

    // *** FIX IS HERE: Updated the file path ***
    match read_geojson("data/natural-earth-vector-master/geojson/ne_110m_coastline.geojson") {
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
                                    &RGBColor(0, 0, 0), // Black coastlines
                                ))?;
                            }
                            Value::MultiLineString(multi_lines) => {
                                for lines_segment in multi_lines {
                                    chart.draw_series(LineSeries::new(
                                        lines_segment
                                            .into_iter()
                                            .map(|line_coord| (line_coord[0], line_coord[1])),
                                        &RGBColor(0, 0, 0), // Black coastlines
                                    ))?;
                                }
                            }
                            _ => { /* Ignore other geometry types for now */ }
                        }
                    }
                }
            }
        }
        Err(e) => eprintln!("Error reading GeoJSON: {}", e),
    }

    root.present()?;
    println!("Plot generated to world_coastlines.png");

    Ok(())
}
