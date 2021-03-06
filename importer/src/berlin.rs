use crate::utils::{download, download_kml, osmconvert};
use abstutil::{prettyprint_usize, Timer};
use geom::Polygon;
use kml::ExtraShapes;
use rand::{Rng, SeedableRng};
use rand_xorshift::XorShiftRng;
use serde::Deserialize;
use std::fs::File;

fn input() {
    download(
        "input/berlin/osm/berlin-latest.osm.pbf",
        "http://download.geofabrik.de/europe/germany/berlin-latest.osm.pbf",
    );

    let bounds = geom::GPSBounds::from(
        geom::LonLat::read_osmosis_polygon(abstutil::path(
            "input/berlin/polygons/berlin_center.poly",
        ))
        .unwrap(),
    );
    // From https://data.technologiestiftung-berlin.de/dataset/lor_planungsgraeume/en
    download_kml(
        "input/berlin/planning_areas.bin",
        "https://tsb-opendata.s3.eu-central-1.amazonaws.com/lor_planungsgraeume/lor_planungsraeume.kml",
        &bounds,
        // Keep partly out-of-bounds polygons
        false,
    );

    // From
    // https://daten.berlin.de/datensaetze/einwohnerinnen-und-einwohner-berlin-lor-planungsr%C3%A4umen-am-31122018
    download(
        "input/berlin/EWR201812E_Matrix.csv",
        "https://www.statistik-berlin-brandenburg.de/opendata/EWR201812E_Matrix.csv",
    );

    // Always do this, it's idempotent and fast
    correlate_population(
        "data/input/berlin/planning_areas.bin",
        "data/input/berlin/EWR201812E_Matrix.csv",
    );
}

pub fn osm_to_raw(name: &str) {
    input();
    osmconvert(
        "input/berlin/osm/berlin-latest.osm.pbf",
        format!("input/berlin/polygons/{}.poly", name),
        format!("input/berlin/osm/{}.osm", name),
    );

    println!("- Running convert_osm");
    let map = convert_osm::convert(
        convert_osm::Options {
            osm_input: abstutil::path(format!("input/berlin/osm/{}.osm", name)),
            city_name: "berlin".to_string(),
            name: name.to_string(),

            clip: Some(abstutil::path(format!(
                "input/berlin/polygons/{}.poly",
                name
            ))),
            map_config: map_model::MapConfig {
                driving_side: map_model::raw::DrivingSide::Right,
                bikes_can_use_bus_lanes: true,
            },

            onstreet_parking: convert_osm::OnstreetParking::JustOSM,
            public_offstreet_parking: convert_osm::PublicOffstreetParking::None,
            private_offstreet_parking: convert_osm::PrivateOffstreetParking::FixedPerBldg(3),
            elevation: None,
        },
        &mut abstutil::Timer::throwaway(),
    );
    let output = abstutil::path(format!("input/raw_maps/{}.bin", name));
    println!("- Saving {}", output);
    abstutil::write_binary(output, &map);
}

// Modify the filtered KML of planning areas with the number of residents from a different dataset.
fn correlate_population(kml_path: &str, csv_path: &str) {
    let mut shapes =
        abstutil::read_binary::<ExtraShapes>(kml_path.to_string(), &mut Timer::throwaway());
    for rec in csv::ReaderBuilder::new()
        .delimiter(b';')
        .from_reader(File::open(csv_path).unwrap())
        .deserialize()
    {
        let rec: Record = rec.unwrap();
        for shape in &mut shapes.shapes {
            if shape.attributes.get("spatial_name") == Some(&rec.raumid) {
                shape
                    .attributes
                    .insert("num_residents".to_string(), rec.e_e);
                break;
            }
        }
    }
    abstutil::write_binary(kml_path.to_string(), &shapes);
}

#[derive(Debug, Deserialize)]
struct Record {
    // Corresponds with spatial_name from planning_areas
    #[serde(rename = "RAUMID")]
    raumid: String,
    // The total residents in that area
    #[serde(rename = "E_E")]
    e_e: String,
}

pub fn distribute_residents(map: &mut map_model::Map, timer: &mut Timer) {
    for shape in abstutil::read_binary::<ExtraShapes>(
        "data/input/berlin/planning_areas.bin".to_string(),
        timer,
    )
    .shapes
    {
        let pts = map.get_gps_bounds().convert(&shape.points);
        if pts
            .iter()
            .all(|pt| !map.get_boundary_polygon().contains_pt(*pt))
        {
            continue;
        }
        let region = Polygon::new(&pts);
        let bldgs: Vec<map_model::BuildingID> = map
            .all_buildings()
            .into_iter()
            .filter(|b| region.contains_pt(b.label_center) && b.bldg_type.has_residents())
            .map(|b| b.id)
            .collect();
        let num_residents = shape.attributes["num_residents"].parse::<usize>().unwrap();
        timer.note(format!(
            "Distributing {} residents in {} to {} buildings",
            prettyprint_usize(num_residents),
            shape.attributes["spatial_alias"],
            prettyprint_usize(bldgs.len())
        ));

        // Deterministically seed using the planning area's ID.
        let mut rng =
            XorShiftRng::seed_from_u64(shape.attributes["spatial_name"].parse::<u64>().unwrap());

        // How do you randomly distribute num_residents into some buildings?
        // https://stackoverflow.com/questions/2640053/getting-n-random-numbers-whose-sum-is-m
        // TODO Problems:
        // - If the region is partly out-of-bounds, then the full number of residents is matched to
        //   a small set of buildings.
        // - Because of how we round, the sum might not exactly be num_residents
        // - This is not a uniform distribution, per stackoverflow
        // - Larger buildings should get more people

        let mut rand_nums: Vec<f64> = (0..bldgs.len()).map(|_| rng.gen_range(0.0, 1.0)).collect();
        let sum: f64 = rand_nums.iter().sum();
        for b in bldgs {
            let n = (rand_nums.pop().unwrap() / sum * (num_residents as f64)) as usize;
            let bldg_type = match map.get_b(b).bldg_type {
                map_model::BuildingType::Residential(_) => map_model::BuildingType::Residential(n),
                map_model::BuildingType::ResidentialCommercial(_) => {
                    map_model::BuildingType::ResidentialCommercial(n)
                }
                _ => unreachable!(),
            };
            map.hack_override_bldg_type(b, bldg_type);
        }
    }

    map.save();
}
