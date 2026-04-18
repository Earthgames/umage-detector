use std::{collections::HashMap, env::current_dir, path::{Path, PathBuf}};

use image::{ImageBuffer, Luma, Rgb, RgbImage};
use imageproc::{distance_transform::Norm, edges::canny, morphology::dilate, region_labelling::{Connectivity, connected_components}};
use rand::RngExt;

#[derive(Debug)]
struct AppArgs {
    debug: bool,
    batch: bool,
    path: PathBuf,
    low_threshold: f32,
    high_threshold: f32,
    output_path: PathBuf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct Rectangle {
    min_x: u32,
    min_y: u32,
    max_x: u32,
    max_y: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct UIBox {
    label: u32,
    rect: Rectangle,
}

fn main() {
    let args = match parse_args() {
        Ok(v) => v,
        Err(e) => {
            eprintln!("Error: {}.", e);
            std::process::exit(1);
        }
    };

    if args.batch {
        process_batch(&args);
    } else {
        process_single_image(&args);
    }
}

fn process_batch(args: &AppArgs) {
    if !args.path.is_dir() {
        eprintln!("Error: Path is not a directory: {:?}", args.path);
        std::process::exit(1);
    }

    let entries = match std::fs::read_dir(&args.path) {
        Ok(entries) => entries,
        Err(e) => {
            eprintln!("Error reading directory: {}", e);
            std::process::exit(1);
        }
    };

    for entry in entries {
        let entry = match entry {
            Ok(e) => e,
            Err(e) => {
                eprintln!("Error reading entry: {}", e);
                continue;
            }
        };

        let path = entry.path();
        if path.is_file() {
            if let Some(extension) = path.extension() {
                let ext_str = extension.to_string_lossy().to_lowercase();
                if ext_str == "png" || ext_str == "jpg" || ext_str == "jpeg" {
                    println!("Processing: {:?}", path);
                    process_single_file(&path, args.debug, args.low_threshold, args.high_threshold, &args.output_path);
                }
            }
        }
    }
}

fn process_single_file(path: &Path, debug: bool, low_threshold: f32, high_threshold: f32, output_path: &Path) {
    let image = match image::open(path) {
        Ok(img) => img,
        Err(e) => {
            eprintln!("Failed to open image {:?}: {}", path, e);
            return;
        }
    };

    let gray = image.to_luma8();
    let edges = canny(&gray, low_threshold, high_threshold);
    let edges = dilate(&edges, Norm::L1, 2);
    let components = connected_components(&edges, Connectivity::Eight, image::Luma([0]));

    if debug {
        debug_components(&components, output_path, path);
    }

    let mut bounds = HashMap::<u32, Rectangle>::new();

    for (x, y, pixel) in components.enumerate_pixels() {
        let label = pixel[0];

        if label == 0 {
            continue;
        }

        bounds
            .entry(label)
            .and_modify(|b| {
                b.min_x = b.min_x.min(x);
                b.min_y = b.min_y.min(y);
                b.max_x = b.max_x.max(x);
                b.max_y = b.max_y.max(y);
            })
            .or_insert(Rectangle {
                min_x: x,
                min_y: y,
                max_x: x,
                max_y: y,
            });
    }

    let mut ui_box = None;
    for (label, rect) in bounds.iter() {
        let width = rect.max_x - rect.min_x;
        let height = rect.max_y - rect.min_y;
        let aspect_ratio = width as f32 / height as f32;

        if width > 200 && height > 300 && (0.55..0.65).contains(&aspect_ratio) {
            if ui_box.is_some() {
                println!("already have a ui box: {:?}, new box: {:?}", ui_box.clone().unwrap(), label);
                continue;
            }
            ui_box = Some(UIBox { label: *label, rect: *rect });
        }
    }

    if let Some(ui_box) = ui_box {
        let rect = bounds.get(&ui_box.label).unwrap();
        let cropped = image.crop_imm(rect.min_x, rect.min_y, rect.max_x - rect.min_x, rect.max_y - rect.min_y);
        
        let output_path = get_output_path(&path, &output_path, "cropped");
        if let Err(e) = cropped.save(&output_path) {
            println!("failed to save cropped image to {:?}: {}", output_path, e);
        } else {
            println!("saved cropped image to {:?}", output_path);
        }
    } else {
        println!("No UI box found in {:?}", path);
    }
}

fn process_single_image(args: &AppArgs) {
    if !args.path.is_file() {
        eprintln!("Error: Path is not a file: {:?}", args.path);
        std::process::exit(1);
    }

    process_single_file(&args.path, args.debug, args.low_threshold, args.high_threshold, &args.output_path);
}

fn parse_args() -> Result<AppArgs, pico_args::Error> {
    let mut pargs = pico_args::Arguments::from_env();

    let debug = pargs.contains(["-d", "--debug"]);
    let batch = pargs.contains(["-b", "--batch"]);
    let path = pargs.free_from_str()?;
    let output_path = pargs.opt_value_from_str(["-o", "--output"])?.unwrap_or(current_dir().unwrap());
    let low_threshold = pargs.opt_value_from_str("--low-threshold")?.unwrap_or(50.0);
    let high_threshold = pargs.opt_value_from_str("--high-threshold")?.unwrap_or(60.0);
    
    println!("{:?}", pargs.finish());

    let args = AppArgs {
        debug,
        batch,
        path,
        low_threshold,
        high_threshold,
        output_path,
    };
    println!("{:?}", args);
    Ok(args)
}

fn get_output_path(path: &Path, output_path: &Path, additional: &str) -> PathBuf {
    let file_name = path.file_prefix().and_then(|n| n.to_str()).unwrap();
    let output_file_name = format!("{}_{}.png", file_name, additional);
    output_path.join(output_file_name)
}

fn debug_components(components: &ImageBuffer<Luma<u32>, Vec<u32>>, output_path: &Path, path: &Path) {
    let mut output = RgbImage::new(components.width(), components.height());
    let mut colors: HashMap<u32, Rgb<u8>> = HashMap::new();
    let mut rng = rand::rng();
    for (x, y, pixel) in components.enumerate_pixels() {
        let label = pixel[0];

        if label == 0 {
            output.put_pixel(x, y, Rgb([0, 0, 0]));
            continue;
        }

        let color = colors.entry(label).or_insert_with(|| {
            Rgb([
                rng.random::<u8>(),
                rng.random::<u8>(),
                rng.random::<u8>(),
            ])
        });

        output.put_pixel(x, y, *color);
    }
    output.save(get_output_path(path, output_path, "components_debug")).unwrap();
}
